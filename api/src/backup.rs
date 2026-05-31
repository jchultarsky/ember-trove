//! Backup and restore service.
//!
//! Archives are stored in S3 under `backups/<job-id>.tar.gz`.
//! Each archive contains:
//! - `manifest.json` — metadata and entity counts
//! - `data.json`     — all serialised entities
//! - `attachments/<node-uuid>/<filename>` — raw attachment bytes

use bytes::Bytes;
use common::{
    backup::{BackupData, BackupJob, BackupManifest, BackupPreview, EntityCounts},
    id::NodeId,
};
use flate2::{Compression, write::GzEncoder};
use std::io::Write;
use tar::Builder;
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

// ── Internal helpers ───────────────────────────────────────────────────────────

fn add_bytes_to_archive<W: Write>(
    builder: &mut Builder<W>,
    path: &str,
    data: &[u8],
) -> Result<(), ApiError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, path, data)
        .map_err(|e| ApiError::Internal(format!("tar append failed for {path}: {e}")))?;
    Ok(())
}

// ── create_backup ──────────────────────────────────────────────────────────────

/// Collect all owner data, pack it into a tar.gz archive, upload to S3,
/// and record the job in `backup_jobs`.
pub async fn create_backup(
    state: &AppState,
    owner_id: &str,
    comment: Option<&str>,
) -> Result<BackupJob, ApiError> {
    // Fetch all data across all users.
    let nodes = state.nodes.list_all().await.map_err(ApiError::from)?;
    let tags = state.tags.list_all().await.map_err(ApiError::from)?;
    let notes = state.notes.list_all().await.map_err(ApiError::from)?;
    let tasks = state.tasks.list_all().await.map_err(ApiError::from)?;
    let edges = state.edges.list_all().await.map_err(ApiError::from)?;

    // Collect every attachment in one query (was an N+1 loop over nodes).
    let attachments = state.attachments.list_all().await.map_err(ApiError::from)?;

    // Collect additional entity types (schema v2).
    let node_links = state.node_links.list_all().await.map_err(ApiError::from)?;
    let favorites = state.favorites.list_all().await.map_err(ApiError::from)?;
    let permissions = state.permissions.list_all(None).await.map_err(ApiError::from)?;
    let share_tokens = state.share_tokens.list_all().await.map_err(ApiError::from)?;
    let node_versions = state.node_versions.list_all().await.map_err(ApiError::from)?;
    // Full backup is admin-only — None = all owners' positions.
    let node_positions = state.graph.list_positions(None).await.map_err(ApiError::from)?;

    let entity_counts = EntityCounts {
        nodes: nodes.len() as u32,
        edges: edges.len() as u32,
        tags: tags.len() as u32,
        notes: notes.len() as u32,
        tasks: tasks.len() as u32,
        attachments: attachments.len() as u32,
    };

    let now = chrono::Utc::now();
    let job_id = Uuid::new_v4();

    let manifest = BackupManifest {
        schema_version: 2,
        created_at: now,
        created_by: owner_id.to_string(),
        entity_counts: entity_counts.clone(),
    };

    let data = BackupData {
        nodes,
        edges,
        tags,
        notes,
        tasks,
        attachments: attachments.clone(),
        node_links,
        favorites,
        permissions,
        share_tokens,
        node_versions,
        node_positions,
    };

    // Build the tar.gz archive in memory.
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| ApiError::Internal(format!("manifest serialise failed: {e}")))?;
    let data_bytes = serde_json::to_vec_pretty(&data)
        .map_err(|e| ApiError::Internal(format!("data serialise failed: {e}")))?;

    let gz_buf: Vec<u8> = Vec::new();
    let enc = GzEncoder::new(gz_buf, Compression::default());
    let mut builder = Builder::new(enc);

    add_bytes_to_archive(&mut builder, "manifest.json", &manifest_bytes)?;
    add_bytes_to_archive(&mut builder, "data.json", &data_bytes)?;

    // Stream attachment bytes into the archive.
    for att in &attachments {
        match state.object_store.get(&att.s3_key).await {
            Ok(file_bytes) => {
                let archive_path = format!(
                    "attachments/{}/{}",
                    att.node_id.0, att.filename
                );
                add_bytes_to_archive(&mut builder, &archive_path, &file_bytes)?;
            }
            Err(e) => {
                tracing::warn!(
                    key = %att.s3_key,
                    "attachment fetch failed during backup, skipping: {e}"
                );
            }
        }
    }

    let enc = builder
        .into_inner()
        .map_err(|e| ApiError::Internal(format!("tar finish failed: {e}")))?;
    let gz_bytes = enc
        .finish()
        .map_err(|e| ApiError::Internal(format!("gzip finish failed: {e}")))?;

    let size_bytes = gz_bytes.len() as i64;
    let s3_key = format!("backups/{job_id}.tar.gz");

    state
        .object_store
        .put(&s3_key, Bytes::from(gz_bytes), "application/gzip")
        .await
        .map_err(|e| ApiError::Storage(format!("backup upload failed: {e}")))?;

    let job = state
        .backup
        .create(
            owner_id,
            &s3_key,
            size_bytes,
            entity_counts.nodes as i32,
            entity_counts.edges as i32,
            entity_counts.tags as i32,
            entity_counts.notes as i32,
            entity_counts.tasks as i32,
            entity_counts.attachments as i32,
            comment,
        )
        .await
        .map_err(ApiError::from)?;

    Ok(job)
}

// ── preview_restore ────────────────────────────────────────────────────────────

/// Download and parse the backup archive, returning a preview without executing
/// any changes.
///
/// `_owner_id` is the sub of the admin previewing the restore.  It is no
/// longer used to restrict which backups the caller may preview — any
/// admin may preview any backup — but is retained in the signature for
/// symmetry with `execute_restore` and potential future auditing.
pub async fn preview_restore(
    state: &AppState,
    job_id: Uuid,
    _owner_id: &str,
) -> Result<BackupPreview, ApiError> {
    let job = state.backup.get(job_id).await.map_err(ApiError::from)?;

    let archive_bytes = state
        .object_store
        .get(&job.s3_key)
        .await
        .map_err(|e| ApiError::Storage(format!("download backup failed: {e}")))?;

    let manifest = extract_manifest(&archive_bytes)?;

    let mut warnings = Vec::new();

    // Warn about current data that will be replaced.
    let current_nodes = state.nodes.list_all().await.map_err(ApiError::from)?;
    if !current_nodes.is_empty() {
        warnings.push(format!(
            "{} existing node(s) and all associated data will be deleted before restore.",
            current_nodes.len()
        ));
    }

    warnings.push(
        "A pre-restore snapshot backup will be created automatically before any changes are made."
            .to_string(),
    );

    Ok(BackupPreview {
        job_id,
        created_at: manifest.created_at,
        entity_counts: manifest.entity_counts,
        warnings,
    })
}

// ── execute_restore ────────────────────────────────────────────────────────────

/// Execute a restore:
/// 1. Create an automatic pre-restore snapshot attributed to the admin
///    who triggered the restore.
/// 2. Download and parse the backup archive.
/// 3. Inside a DB transaction: delete existing data, insert backup data.
/// 4. Re-upload attachment files to S3 (best-effort).
///
/// `owner_id` is the sub of the admin running the restore — used only as
/// the `created_by` field on the pre-restore snapshot so an audit trail
/// records who initiated the destructive operation.  It does not restrict
/// which backups the caller may restore: any admin may restore any
/// backup.
pub async fn execute_restore(
    state: &AppState,
    job_id: Uuid,
    owner_id: &str,
) -> Result<(), ApiError> {
    let job = state.backup.get(job_id).await.map_err(ApiError::from)?;

    // Auto snapshot before restore, attributed to the admin who triggered it.
    create_backup(state, owner_id, Some("auto: pre-restore snapshot")).await?;

    let archive_bytes = state
        .object_store
        .get(&job.s3_key)
        .await
        .map_err(|e| ApiError::Storage(format!("download backup failed: {e}")))?;

    let (manifest, data, attachment_files) = extract_full(&archive_bytes)?;

    if manifest.schema_version > 2 {
        return Err(ApiError::Internal(format!(
            "unsupported backup schema version: {} (max supported: 2)",
            manifest.schema_version
        )));
    }

    // Run all DB changes in a single transaction.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::Internal(format!("begin transaction failed: {e}")))?;

    // Delete all nodes (CASCADE removes edges, notes, tasks, attachments, permissions).
    sqlx::query("DELETE FROM nodes")
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("delete nodes failed: {e}")))?;

    // Delete all tags.
    sqlx::query("DELETE FROM tags")
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("delete tags failed: {e}")))?;

    // Delete user favorites (both node-based and URL-based — we'll restore all).
    sqlx::query("DELETE FROM user_favorites")
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("delete favorites failed: {e}")))?;

    // Insert tags first (nodes may reference them via node_tags).
    for tag in &data.tags {
        sqlx::query(
            r#"
            INSERT INTO tags (id, owner_id, name, color, created_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE
                SET name = EXCLUDED.name,
                    color = EXCLUDED.color
            "#,
        )
        .bind(tag.id.0)
        .bind(&tag.owner_id)
        .bind(&tag.name)
        .bind(&tag.color)
        .bind(tag.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert tag failed: {e}")))?;
    }

    // Insert nodes.
    for node in &data.nodes {
        let node_type_str = node_type_to_str(&node.node_type);
        let status_str = node_status_to_str(&node.status);
        sqlx::query(
            r#"
            INSERT INTO nodes
                (id, owner_id, node_type, title, slug, body, metadata, status, pinned, created_at, updated_at)
            VALUES ($1, $2, $3::node_type, $4, $5, $6, $7, $8::node_status, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE
                SET title = EXCLUDED.title,
                    body  = EXCLUDED.body,
                    slug  = EXCLUDED.slug,
                    metadata = EXCLUDED.metadata,
                    status = EXCLUDED.status,
                    pinned = EXCLUDED.pinned
            "#,
        )
        .bind(node.id.0)
        .bind(&node.owner_id)
        .bind(node_type_str)
        .bind(&node.title)
        .bind(&node.slug)
        .bind(&node.body)
        .bind(&node.metadata)
        .bind(status_str)
        .bind(node.pinned)
        .bind(node.created_at)
        .bind(node.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert node failed: {e}")))?;

        // Re-attach tags.
        for tag in &node.tags {
            sqlx::query(
                "INSERT INTO node_tags (node_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(node.id.0)
            .bind(tag.id.0)
            .execute(&mut *tx)
            .await
            .map_err(|e| ApiError::Internal(format!("insert node_tag failed: {e}")))?;
        }
    }

    // Insert edges.
    for edge in &data.edges {
        let edge_type_str = edge_type_to_str(&edge.edge_type);
        sqlx::query(
            r#"
            INSERT INTO edges (id, source_id, target_id, edge_type, label, created_at)
            VALUES ($1, $2, $3, $4::edge_type, $5, $6)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(edge.id.0)
        .bind(edge.source_id.0)
        .bind(edge.target_id.0)
        .bind(edge_type_str)
        .bind(&edge.label)
        .bind(edge.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert edge failed: {e}")))?;
    }

    // Insert notes.
    for note in &data.notes {
        sqlx::query(
            r#"
            INSERT INTO node_notes (id, node_id, owner_id, body, created_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(note.id.0)
        .bind(note.node_id.map(|n| n.0))
        .bind(&note.owner_id)
        .bind(&note.body)
        .bind(note.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert note failed: {e}")))?;
    }

    // Insert tasks.
    for task in &data.tasks {
        let status_str = task_status_to_str(&task.status);
        let priority_str = task_priority_to_str(&task.priority);
        let recurrence_str = task.recurrence.as_ref().map(recurrence_rule_to_str);
        sqlx::query(
            r#"
            INSERT INTO node_tasks
                (id, node_id, owner_id, title, status, priority, focus_date, due_date,
                 recurrence, sort_order, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5::task_status, $6::task_priority, $7, $8,
                    $9, $10, $11, $12)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(task.id.0)
        .bind(task.node_id.map(|n| n.0))
        .bind(&task.owner_id)
        .bind(&task.title)
        .bind(status_str)
        .bind(priority_str)
        .bind(task.focus_date)
        .bind(task.due_date)
        .bind(recurrence_str)
        .bind(task.sort_order)
        .bind(task.created_at)
        .bind(task.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert task failed: {e}")))?;
    }

    // Insert attachment metadata rows.
    for att in &data.attachments {
        sqlx::query(
            r#"
            INSERT INTO attachments
                (id, node_id, filename, content_type, size_bytes, s3_key, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(att.id.0)
        .bind(att.node_id.0)
        .bind(&att.filename)
        .bind(&att.content_type)
        .bind(att.size_bytes)
        .bind(&att.s3_key)
        .bind(att.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert attachment metadata failed: {e}")))?;
    }

    // ── Schema v2 entities ──────────────────────────────────────────────────

    // Insert node links.
    for link in &data.node_links {
        sqlx::query(
            r#"
            INSERT INTO node_links (id, node_id, name, url, created_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(link.id.0)
        .bind(link.node_id.0)
        .bind(&link.name)
        .bind(&link.url)
        .bind(link.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert node_link failed: {e}")))?;
    }

    // Insert favorites.
    for fav in &data.favorites {
        sqlx::query(
            r#"
            INSERT INTO user_favorites (id, owner_id, node_id, url, label, position, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(fav.id.0)
        .bind(&fav.owner_id)
        .bind(fav.node_id.map(|n| n.0))
        .bind(&fav.url)
        .bind(&fav.label)
        .bind(fav.position)
        .bind(fav.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert favorite failed: {e}")))?;
    }

    // Insert permissions.
    for perm in &data.permissions {
        let role_str = permission_role_to_str(&perm.role);
        sqlx::query(
            r#"
            INSERT INTO permissions (id, node_id, subject_id, role, granted_by, created_at)
            VALUES ($1, $2, $3, $4::permission_role, $5, $6)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(perm.id.0)
        .bind(perm.node_id.0)
        .bind(&perm.subject_id)
        .bind(role_str)
        .bind(&perm.granted_by)
        .bind(perm.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert permission failed: {e}")))?;
    }

    // Insert share tokens.
    for token in &data.share_tokens {
        sqlx::query(
            r#"
            INSERT INTO share_tokens (id, node_id, token, created_by, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(token.id.0)
        .bind(token.node_id.0)
        .bind(token.token)
        .bind(&token.created_by)
        .bind(token.created_at)
        .bind(token.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert share_token failed: {e}")))?;
    }

    // Insert node versions.
    for ver in &data.node_versions {
        sqlx::query(
            r#"
            INSERT INTO node_versions (id, node_id, body, created_by, created_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(ver.id.0)
        .bind(ver.node_id.0)
        .bind(&ver.body)
        .bind(&ver.created_by)
        .bind(ver.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert node_version failed: {e}")))?;
    }

    // Insert graph positions.
    for pos in &data.node_positions {
        sqlx::query(
            r#"
            INSERT INTO node_positions (node_id, x, y)
            VALUES ($1, $2, $3)
            ON CONFLICT (node_id) DO UPDATE SET x = EXCLUDED.x, y = EXCLUDED.y
            "#,
        )
        .bind(pos.node_id.0)
        .bind(pos.x)
        .bind(pos.y)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("insert node_position failed: {e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| ApiError::Internal(format!("commit restore transaction failed: {e}")))?;

    // Re-upload attachment files to S3 (best-effort — non-fatal).
    for (archive_path, file_bytes) in &attachment_files {
        // Determine the s3_key from the matching attachment metadata.
        let s3_key = data
            .attachments
            .iter()
            .find(|a| {
                archive_path == &format!("attachments/{}/{}", a.node_id.0, a.filename)
            })
            .map(|a| a.s3_key.clone());

        if let Some(key) = s3_key {
            let content_type = data
                .attachments
                .iter()
                .find(|a| a.s3_key == key)
                .map(|a| a.content_type.as_str())
                .unwrap_or("application/octet-stream");

            if let Err(e) = state
                .object_store
                .put(&key, Bytes::copy_from_slice(file_bytes), content_type)
                .await
            {
                tracing::warn!(
                    key = %key,
                    "re-upload attachment to S3 failed (non-fatal): {e}"
                );
            }
        }
    }

    Ok(())
}

// ── Archive parsing helpers ────────────────────────────────────────────────────

fn extract_manifest(archive_bytes: &[u8]) -> Result<BackupManifest, ApiError> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| ApiError::Internal(format!("read archive entries failed: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| ApiError::Internal(format!("read archive entry failed: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| ApiError::Internal(format!("archive entry path failed: {e}")))?
            .to_string_lossy()
            .to_string();
        if path == "manifest.json" {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| ApiError::Internal(format!("read manifest.json failed: {e}")))?;
            let manifest: BackupManifest = serde_json::from_slice(&buf)
                .map_err(|e| ApiError::Internal(format!("parse manifest.json failed: {e}")))?;
            return Ok(manifest);
        }
    }

    Err(ApiError::Internal(
        "manifest.json not found in backup archive".to_string(),
    ))
}

/// Parse the archive and return (manifest, data, attachment_files).
/// `attachment_files` is a Vec of (archive_path, raw_bytes).
#[allow(clippy::type_complexity)]
fn extract_full(
    archive_bytes: &[u8],
) -> Result<(BackupManifest, BackupData, Vec<(String, Vec<u8>)>), ApiError> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| ApiError::Internal(format!("read archive entries failed: {e}")))?;

    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut data_bytes: Option<Vec<u8>> = None;
    let mut attachment_files: Vec<(String, Vec<u8>)> = Vec::new();

    for entry in entries {
        let mut entry =
            entry.map_err(|e| ApiError::Internal(format!("read archive entry failed: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| ApiError::Internal(format!("archive entry path failed: {e}")))?
            .to_string_lossy()
            .to_string();

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| ApiError::Internal(format!("read entry '{path}' failed: {e}")))?;

        if path == "manifest.json" {
            manifest_bytes = Some(buf);
        } else if path == "data.json" {
            data_bytes = Some(buf);
        } else if path.starts_with("attachments/") {
            attachment_files.push((path, buf));
        }
    }

    let manifest: BackupManifest = serde_json::from_slice(
        &manifest_bytes.ok_or_else(|| {
            ApiError::Internal("manifest.json missing from archive".to_string())
        })?,
    )
    .map_err(|e| ApiError::Internal(format!("parse manifest.json failed: {e}")))?;

    let data: BackupData = serde_json::from_slice(
        &data_bytes
            .ok_or_else(|| ApiError::Internal("data.json missing from archive".to_string()))?,
    )
    .map_err(|e| ApiError::Internal(format!("parse data.json failed: {e}")))?;

    Ok((manifest, data, attachment_files))
}

// ── Type-conversion helpers (duplicated from repo to avoid pub re-export) ──────

fn node_type_to_str(t: &common::node::NodeType) -> &'static str {
    match t {
        common::node::NodeType::Article => "article",
        common::node::NodeType::Project => "project",
        common::node::NodeType::Area => "area",
        common::node::NodeType::Resource => "resource",
        common::node::NodeType::Reference => "reference",
    }
}

fn node_status_to_str(s: &common::node::NodeStatus) -> &'static str {
    match s {
        common::node::NodeStatus::Draft => "draft",
        common::node::NodeStatus::Published => "published",
        common::node::NodeStatus::Archived => "archived",
    }
}

fn edge_type_to_str(t: &common::edge::EdgeType) -> &'static str {
    match t {
        common::edge::EdgeType::References => "references",
        common::edge::EdgeType::Contains => "contains",
        common::edge::EdgeType::RelatedTo => "related_to",
        common::edge::EdgeType::DependsOn => "depends_on",
        common::edge::EdgeType::DerivedFrom => "derived_from",
        common::edge::EdgeType::WikiLink => "wiki_link",
    }
}

fn task_status_to_str(s: &common::task::TaskStatus) -> &'static str {
    match s {
        common::task::TaskStatus::Open => "open",
        common::task::TaskStatus::InProgress => "in_progress",
        common::task::TaskStatus::Done => "done",
        common::task::TaskStatus::Cancelled => "cancelled",
    }
}

fn task_priority_to_str(p: &common::task::TaskPriority) -> &'static str {
    match p {
        common::task::TaskPriority::Low => "low",
        common::task::TaskPriority::Medium => "medium",
        common::task::TaskPriority::High => "high",
    }
}

fn permission_role_to_str(r: &common::permission::PermissionRole) -> &'static str {
    match r {
        common::permission::PermissionRole::Owner => "owner",
        common::permission::PermissionRole::Editor => "editor",
        common::permission::PermissionRole::Viewer => "viewer",
    }
}

fn recurrence_rule_to_str(r: &common::task::RecurrenceRule) -> &'static str {
    match r {
        common::task::RecurrenceRule::Daily => "daily",
        common::task::RecurrenceRule::Weekly => "weekly",
        common::task::RecurrenceRule::Biweekly => "biweekly",
        common::task::RecurrenceRule::Monthly => "monthly",
        common::task::RecurrenceRule::Yearly => "yearly",
    }
}

// Suppress unused import warning — NodeId is used in type annotation context via the trait.
const _: fn() = || {
    let _: NodeId = NodeId(Uuid::nil());
};
