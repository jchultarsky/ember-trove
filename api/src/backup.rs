//! Backup and restore service.
//!
//! Archives are stored in S3 under `backups/<job-id>.tar.gz`.
//! Each archive contains:
//! - `manifest.json` — metadata and entity counts
//! - `data.json`     — all serialised entities
//! - `attachments/<node-uuid>/<filename>` — raw attachment bytes

use bytes::Bytes;
use common::{
    activity::ActivityRecord,
    backup::{BackupData, BackupJob, BackupManifest, BackupPreview, EntityCounts},
    id::NodeId,
};
use flate2::{Compression, write::GzEncoder};
use std::collections::HashSet;
use std::io::Write;
use tar::Builder;
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

// ── Internal helpers ───────────────────────────────────────────────────────────

/// A node's body and project status as restored (#143). A backup from
/// before v2.36.0 has a project's status as prose in its body; its token
/// moves into the field, as the 035 backfill did on live data, or it would
/// come back as prose. An invalid stored status is dropped rather than
/// failing the whole restore on the column's CHECK.
fn restored_project_status(node: &common::node::Node) -> (Option<String>, Option<String>) {
    use common::project_status::{normalize, split_from_body};
    if let Some(ps) = &node.project_status {
        return (node.body.clone(), normalize(ps).ok().flatten());
    }
    if node.node_type == common::node::NodeType::Project
        && let Some(split) = node.body.as_deref().and_then(split_from_body)
    {
        return (Some(split.body), Some(split.status));
    }
    (node.body.clone(), None)
}

/// Restoring replaces the whole activity log (#208). An archive without one
/// (made before backups carried it, or from an empty log) would leave every
/// node's Activity empty, and the owner should know before confirming.
fn activity_warning(counts: &EntityCounts) -> Option<String> {
    (counts.activity == 0).then(|| {
        "This backup has no activity history: after the restore, every node's Activity \
         list will be empty."
            .to_string()
    })
}

/// Only archives in this release's format restore (#222). Checked before
/// anything else, so an older archive costs no pre-restore snapshot and
/// never reaches the parser, which would fail on its data with a 500.
fn check_format(manifest: &BackupManifest) -> Result<(), ApiError> {
    let wanted = common::backup::SCHEMA_VERSION;
    match manifest.schema_version {
        v if v == wanted => Ok(()),
        v if v < wanted => Err(ApiError::Validation(format!(
            "This backup was made by an older version of Ember Trove (format {v}). \
             This release restores format {wanted} only."
        ))),
        v => Err(ApiError::Validation(format!(
            "This backup was made by a newer version of Ember Trove (format {v}). \
             This release restores format {wanted} only."
        ))),
    }
}

/// The restore preview's warning about links in the archive that break the
/// data model (#224): counts per Data health rule, never titles or ids.
/// `None` when every link fits.
fn edge_rules_warning(data: &BackupData) -> Option<String> {
    let kinds = data
        .nodes
        .iter()
        .map(|n| (n.id.0, n.node_type.clone()))
        .collect();
    let edges: Vec<_> = data
        .edges
        .iter()
        .map(|e| (e.source_id.0, e.target_id.0, e.edge_type.clone()))
        .collect();
    rules_warning(&common::data_health::graph_rule_counts(&kinds, &edges))
}

fn rules_warning(counts: &[(common::data_health::RuleId, usize)]) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let list = counts
        .iter()
        .map(|(rule, n)| format!("{} ({n})", rule.title()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "Some links in this backup break the data model: {list}. \
         They are restored as they are, and the Data health page lists them."
    ))
}

/// The restore preview's notes on what restore folds (#226, #228): related
/// links stored both ways become one (041's rule), and a node with several
/// homes keeps its oldest (042's). Folded links are kept aside in
/// `edges_removed_041` / `_042`. Counts only.
fn folded_links_warnings(data: &BackupData) -> Vec<String> {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let (edges, merged) = common::edge::fold_related_pairs(data.edges.clone());
    let (_, extra_homes) = common::edge::fold_homes(edges);
    let mut out = Vec::new();
    if !merged.is_empty() {
        let n = merged.len();
        out.push(format!(
            "{n} related link{} in this backup {} stored both ways; \
             each pair is restored as one link.",
            plural(n),
            if n == 1 { "is" } else { "are" },
        ));
    }
    if !extra_homes.is_empty() {
        let n = extra_homes.len();
        out.push(format!(
            "{n} home link{} in this backup would give a node a second home; \
             each node keeps its oldest home.",
            plural(n),
        ));
    }
    out
}

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
    let permissions = state
        .permissions
        .list_all(None)
        .await
        .map_err(ApiError::from)?;
    let node_versions = state
        .node_versions
        .list_all()
        .await
        .map_err(ApiError::from)?;
    // Full backup is admin-only — None = all owners' positions.
    let node_positions = state
        .graph
        .list_positions(None)
        .await
        .map_err(ApiError::from)?;

    // People (#147): third-party contact details, all owners (admin backup).
    let (people, node_people) = state.people.list_all().await.map_err(ApiError::from)?;

    // Activity (#208): the History record (ONTOLOGY §10), all owners.
    let activity = state.activity.list_all().await.map_err(ApiError::from)?;

    let entity_counts = EntityCounts {
        nodes: nodes.len() as u32,
        edges: edges.len() as u32,
        tags: tags.len() as u32,
        notes: notes.len() as u32,
        tasks: tasks.len() as u32,
        attachments: attachments.len() as u32,
        people: people.len() as u32,
        activity: activity.len() as u32,
    };

    let now = chrono::Utc::now();
    let job_id = Uuid::new_v4();

    let manifest = BackupManifest {
        schema_version: common::backup::SCHEMA_VERSION,
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
        node_versions,
        node_positions,
        people,
        node_people,
        activity,
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
                let archive_path = format!("attachments/{}/{}", att.node_id.0, att.filename);
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
    // Say so before the admin ever reaches Confirm.
    check_format(&manifest)?;

    let mut warnings = Vec::new();

    // Warn about current data that will be replaced.
    let current_nodes = state.nodes.list_all().await.map_err(ApiError::from)?;
    if !current_nodes.is_empty() {
        warnings.push(format!(
            "{} existing node(s) and all associated data will be deleted before restore.",
            current_nodes.len()
        ));
    }

    if let Some(w) = activity_warning(&manifest.entity_counts) {
        warnings.push(w);
    }
    // Restore never refuses old data (owner decision, #224); it says what
    // the archive's links break, in counts only.
    // An unreadable data.json fails the restore itself later; the preview
    // still shows, saying it couldn't check.
    match extract_data(&archive_bytes) {
        Ok(data) => {
            warnings.extend(edge_rules_warning(&data));
            warnings.extend(folded_links_warnings(&data));
        }
        Err(e) => {
            tracing::warn!("restore preview couldn't read data.json: {e:?}");
            warnings.push(
                "This backup's data couldn't be read to check its links; the restore may fail."
                    .to_string(),
            );
        }
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

    let download = || async {
        state
            .object_store
            .get(&job.s3_key)
            .await
            .map_err(|e| ApiError::Storage(format!("download backup failed: {e}")))
    };

    // The format first (#222): an archive this release can't restore costs
    // no snapshot and never reaches the data parser. The archive is dropped
    // again before the snapshot, which builds a second full archive in
    // memory: holding both could exhaust a small VM mid-restore.
    check_format(&extract_manifest(&download().await?)?)?;

    // Auto snapshot before restore, attributed to the admin who triggered it.
    create_backup(state, owner_id, Some("auto: pre-restore snapshot")).await?;

    let archive_bytes = download().await?;
    let (manifest, data, attachment_files) = extract_full(&archive_bytes)?;
    // Again on the manifest the parse used: an archive with two manifests
    // can't pass the first check with one and restore with the other.
    check_format(&manifest)?;

    // Run all DB changes in a single transaction.
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::Internal(format!("begin transaction failed: {e}")))?;

    restore_into(&mut tx, &data).await?;

    tx.commit()
        .await
        .map_err(|e| ApiError::Internal(format!("commit restore transaction failed: {e}")))?;

    // Re-upload attachment files to S3 (best-effort — non-fatal).
    for (archive_path, file_bytes) in &attachment_files {
        // Determine the s3_key from the matching attachment metadata.
        let s3_key = data
            .attachments
            .iter()
            .find(|a| archive_path == &format!("attachments/{}/{}", a.node_id.0, a.filename))
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

/// Replaces every user's data with an archive's, on the caller's
/// transaction: nothing is visible until it commits, and any failure rolls
/// the whole restore back. Split out of [`execute_restore`] (#208) so the
/// database half of a restore can be tested end to end.
pub(crate) async fn restore_into(
    conn: &mut sqlx::PgConnection,
    data: &BackupData,
) -> Result<(), ApiError> {
    // Delete all nodes (CASCADE removes edges, notes, tasks, attachments, permissions).
    sqlx::query("DELETE FROM nodes")
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("delete nodes failed: {e}")))?;

    // Delete all tags.
    sqlx::query("DELETE FROM tags")
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("delete tags failed: {e}")))?;

    // Delete user favorites (both node-based and URL-based — we'll restore all).
    sqlx::query("DELETE FROM user_favorites")
        .execute(&mut *conn)
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
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert tag failed: {e}")))?;
    }

    // Insert nodes.
    for node in &data.nodes {
        let node_type_str = node.node_type.as_str();
        let status_str = node.status.as_str();
        let (body, project_status) = restored_project_status(node);
        sqlx::query(
            r#"
            INSERT INTO nodes
                (id, owner_id, node_type, title, slug, body, metadata, status, pinned, created_at,
                 updated_at, project_status)
            VALUES ($1, $2, $3::node_type, $4, $5, $6, $7, $8::node_status, $9, $10, $11, $12)
            ON CONFLICT (id) DO UPDATE
                SET title = EXCLUDED.title,
                    body  = EXCLUDED.body,
                    slug  = EXCLUDED.slug,
                    metadata = EXCLUDED.metadata,
                    status = EXCLUDED.status,
                    pinned = EXCLUDED.pinned,
                    project_status = EXCLUDED.project_status
            "#,
        )
        .bind(node.id.0)
        .bind(&node.owner_id)
        .bind(node_type_str)
        .bind(&node.title)
        .bind(&node.slug)
        .bind(&body)
        .bind(&node.metadata)
        .bind(status_str)
        .bind(node.pinned)
        .bind(node.created_at)
        .bind(node.updated_at)
        .bind(&project_status)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert node failed: {e}")))?;

        // Re-attach tags.
        for tag in &node.tags {
            sqlx::query(
                "INSERT INTO node_tags (node_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(node.id.0)
            .bind(tag.id.0)
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("insert node_tag failed: {e}")))?;
        }
    }

    // Insert edges. `related_to` is unordered (#226): an archive from before
    // migration 041 may hold both directions, which the unique index
    // refuses, so they are merged by the migration's rule first.
    // The database holds one related link per pair (041) and one home per
    // node (042); an archive from before may hold more. Fold them by the
    // migrations' own rules and archive the rest the same way, so no label
    // or link is lost (#226, #228).
    let (edges, merged) = common::edge::fold_related_pairs(data.edges.clone());
    let (edges, extra_homes) = common::edge::fold_homes(edges);
    for (table, edge) in merged
        .iter()
        .map(|e| ("edges_removed_041", e))
        .chain(extra_homes.iter().map(|e| ("edges_removed_042", e)))
    {
        // `table` is one of the two literals above, never input.
        sqlx::query(&format!(
            "INSERT INTO {table} (id, source_id, target_id, edge_type, label, created_at) \
             VALUES ($1, $2, $3, $4::edge_type, $5, $6)"
        ))
        .bind(edge.id.0)
        .bind(edge.source_id.0)
        .bind(edge.target_id.0)
        .bind(edge.edge_type.as_str())
        .bind(&edge.label)
        .bind(edge.created_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("archive folded edge failed: {e}")))?;
    }
    for edge in &edges {
        let edge_type_str = edge.edge_type.as_str();
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
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert edge failed: {e}")))?;
    }

    // Insert notes.
    for note in &data.notes {
        sqlx::query(
            r#"
            INSERT INTO node_notes (id, node_id, owner_id, body, color, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        // color + updated_at are restored too: they used to fall back to
        // 'default' / NOW(), losing note colours and edit times.
        .bind(note.id.0)
        .bind(note.node_id.map(|n| n.0))
        .bind(&note.owner_id)
        .bind(&note.body)
        .bind(&note.color)
        .bind(note.created_at)
        .bind(note.updated_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert note failed: {e}")))?;
    }

    // Insert tasks.
    for task in &data.tasks {
        let status_str = task.status.as_str();
        let priority_str = task.priority.as_str();
        let recurrence_str = task.recurrence.as_ref().map(|v| v.as_str());
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
        .execute(&mut *conn)
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
        .execute(&mut *conn)
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
        .execute(&mut *conn)
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
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert favorite failed: {e}")))?;
    }

    // Insert permissions.
    for perm in &data.permissions {
        let role_str = perm.role.as_str();
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
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert permission failed: {e}")))?;
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
        .execute(&mut *conn)
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
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert node_position failed: {e}")))?;
    }

    // Insert people (#147), then their memberships (they need both the
    // node and the person rows). Owners are kept as stored, like nodes.
    for p in &data.people {
        sqlx::query(
            r#"
            INSERT INTO people
                (id, owner_id, display_name, company, position, emails, phones,
                 addresses, notes, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE
                SET display_name = EXCLUDED.display_name, company = EXCLUDED.company,
                    position = EXCLUDED.position, emails = EXCLUDED.emails,
                    phones = EXCLUDED.phones, addresses = EXCLUDED.addresses,
                    notes = EXCLUDED.notes
            "#,
        )
        .bind(p.id.0)
        .bind(&p.owner_id)
        .bind(&p.display_name)
        .bind(&p.company)
        .bind(&p.position)
        .bind(sqlx::types::Json(&p.emails))
        .bind(sqlx::types::Json(&p.phones))
        .bind(sqlx::types::Json(&p.addresses))
        .bind(&p.notes)
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&mut *conn)
        .await
        // Never include the row: it's someone's contact details.
        .map_err(|e| ApiError::Internal(format!("insert person failed: {e}")))?;
    }
    for m in &data.node_people {
        sqlx::query(
            r#"
            INSERT INTO node_people (id, node_id, person_id, role, note, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(m.id.0)
        .bind(m.node_id.0)
        .bind(m.person_id.0)
        .bind(&m.role)
        .bind(&m.note)
        .bind(m.created_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert membership failed: {e}")))?;
    }

    // Activity last (#208): every row needs its node. Deleting the nodes
    // above cascaded the old log away, so this brings History back.
    let restored_nodes: HashSet<Uuid> = data.nodes.iter().map(|n| n.id.0).collect();
    restore_activity(conn, &data.activity, &restored_nodes).await?;

    Ok(())
}

/// Re-inserts a backup's activity rows in one statement (the log only
/// grows, so it's batched rather than one INSERT per row). Rows whose node
/// isn't being restored are skipped: `create_backup` reads each table in its
/// own query, so a node created mid-backup can leave activity behind
/// without its node, and that row would fail the FK and abort the whole
/// restore. Actions are inserted verbatim, known to this build or not.
/// Returns how many rows were inserted.
pub(crate) async fn restore_activity(
    conn: &mut sqlx::PgConnection,
    records: &[ActivityRecord],
    restored_nodes: &HashSet<Uuid>,
) -> Result<u64, ApiError> {
    let kept: Vec<&ActivityRecord> = records
        .iter()
        .filter(|r| restored_nodes.contains(&r.node_id.0))
        .collect();
    let skipped = records.len() - kept.len();
    if skipped > 0 {
        tracing::warn!(
            skipped,
            "restore: skipping activity rows whose node is not in the backup"
        );
    }
    if kept.is_empty() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = kept.iter().map(|r| r.id.0).collect();
    let node_ids: Vec<Uuid> = kept.iter().map(|r| r.node_id.0).collect();
    let subjects: Vec<&str> = kept.iter().map(|r| r.subject_id.as_str()).collect();
    let actions: Vec<&str> = kept.iter().map(|r| r.action.as_str()).collect();
    let metadata: Vec<&serde_json::Value> = kept.iter().map(|r| &r.metadata).collect();
    let created: Vec<chrono::DateTime<chrono::Utc>> = kept.iter().map(|r| r.created_at).collect();
    let done = sqlx::query(
        r#"
        INSERT INTO activity_log (id, node_id, subject_id, action, metadata, created_at)
        SELECT * FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[], $5::jsonb[],
                             $6::timestamptz[])
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(&ids)
    .bind(&node_ids)
    .bind(&subjects)
    .bind(&actions)
    .bind(&metadata)
    .bind(&created)
    .execute(&mut *conn)
    .await
    .map_err(|e| ApiError::Internal(format!("insert activity failed: {e}")))?;
    Ok(done.rows_affected())
}

// ── Archive parsing helpers ────────────────────────────────────────────────────

fn extract_manifest(archive_bytes: &[u8]) -> Result<BackupManifest, ApiError> {
    serde_json::from_slice(&read_entry(archive_bytes, "manifest.json")?)
        .map_err(|e| ApiError::Internal(format!("parse manifest.json failed: {e}")))
}

/// Parse only `data.json`, for the restore preview: attachment entries are
/// skipped unread, so a preview never holds their bytes.
fn extract_data(archive_bytes: &[u8]) -> Result<BackupData, ApiError> {
    serde_json::from_slice(&read_entry(archive_bytes, "data.json")?)
        .map_err(|e| ApiError::Internal(format!("parse data.json failed: {e}")))
}

/// Decompression-bomb guard: the most one archive entry may expand to.
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB per file

/// The bytes of the first entry called `name`, read with the per-entry
/// bound; every other entry is skipped unread.
fn read_entry(archive_bytes: &[u8], name: &str) -> Result<Vec<u8>, ApiError> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let mut archive = tar::Archive::new(GzDecoder::new(archive_bytes));
    let entries = archive
        .entries()
        .map_err(|e| ApiError::Internal(format!("read archive entries failed: {e}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| ApiError::Internal(format!("read archive entry failed: {e}")))?;
        let matches = entry
            .path()
            .map_err(|e| ApiError::Internal(format!("archive entry path failed: {e}")))?
            .as_os_str()
            == name;
        if !matches {
            continue;
        }
        let mut buf = Vec::new();
        (&mut entry)
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| ApiError::Internal(format!("read {name} failed: {e}")))?;
        if buf.len() as u64 > MAX_ENTRY_BYTES {
            return Err(ApiError::Validation(
                "backup archive entry exceeds the size limit".to_string(),
            ));
        }
        return Ok(buf);
    }
    Err(ApiError::Internal(format!(
        "{name} not found in backup archive"
    )))
}

/// Parse the archive and return (manifest, data, attachment_files).
/// `attachment_files` is a Vec of (archive_path, raw_bytes).
#[allow(clippy::type_complexity)]
fn extract_full(
    archive_bytes: &[u8],
) -> Result<(BackupManifest, BackupData, Vec<(String, Vec<u8>)>), ApiError> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    // Decompression-bomb guards: bound per-entry and cumulative extracted size
    // so a malicious/corrupted archive can't OOM the API during restore.
    const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB total

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| ApiError::Internal(format!("read archive entries failed: {e}")))?;

    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut data_bytes: Option<Vec<u8>> = None;
    let mut attachment_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_bytes: u64 = 0;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| ApiError::Internal(format!("read archive entry failed: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| ApiError::Internal(format!("archive entry path failed: {e}")))?
            .to_string_lossy()
            .to_string();

        // Read at most MAX_ENTRY_BYTES + 1 so an oversized entry is detected
        // without materialising the whole thing.
        let mut buf = Vec::new();
        (&mut entry)
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| ApiError::Internal(format!("read entry '{path}' failed: {e}")))?;
        if buf.len() as u64 > MAX_ENTRY_BYTES {
            return Err(ApiError::Validation(
                "backup archive entry exceeds the size limit".to_string(),
            ));
        }
        total_bytes = total_bytes.saturating_add(buf.len() as u64);
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(ApiError::Validation(
                "backup archive exceeds the total size limit".to_string(),
            ));
        }

        if path == "manifest.json" {
            manifest_bytes = Some(buf);
        } else if path == "data.json" {
            data_bytes = Some(buf);
        } else if path.starts_with("attachments/") {
            // Reject path traversal before the key is ever used as an S3 key.
            if path.contains("..") {
                return Err(ApiError::Validation(
                    "backup archive contains an invalid path".to_string(),
                ));
            }
            attachment_files.push((path, buf));
        }
    }

    let manifest: BackupManifest = serde_json::from_slice(
        &manifest_bytes
            .ok_or_else(|| ApiError::Internal("manifest.json missing from archive".to_string()))?,
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

// Suppress unused import warning — NodeId is used in type annotation context via the trait.
const _: fn() = || {
    let _: NodeId = NodeId(Uuid::nil());
};

#[cfg(test)]
mod tests {
    use super::*;
    use common::node::{Node, NodeStatus, NodeType};

    fn node(node_type: NodeType, body: &str, project_status: Option<&str>) -> Node {
        Node {
            id: NodeId(Uuid::new_v4()),
            owner_id: "o".into(),
            node_type,
            title: "t".into(),
            slug: "t".into(),
            body: Some(body.into()),
            metadata: serde_json::json!({}),
            status: NodeStatus::Draft,
            project_status: project_status.map(str::to_string),
            tags: vec![],
            pinned: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            edge_count: 0,
        }
    }

    #[test]
    fn an_old_backups_project_status_moves_out_of_the_body() {
        let n = node(
            NodeType::Project,
            "## Status\n\n- Active\n\n## Goals\n",
            None,
        );
        assert_eq!(
            restored_project_status(&n),
            (Some("## Goals\n".into()), Some("Active".into()))
        );
    }

    #[test]
    fn a_new_backups_status_is_kept_and_an_invalid_one_dropped() {
        let body = "## Status\n\n- Active\n";
        let n = node(NodeType::Project, body, Some("Done"));
        assert_eq!(
            restored_project_status(&n),
            (Some(body.into()), Some("Done".into()))
        );
        let bad = node(NodeType::Project, body, Some(&"x".repeat(41)));
        assert_eq!(restored_project_status(&bad), (Some(body.into()), None));
    }

    #[test]
    fn other_types_are_restored_as_they_were() {
        let n = node(NodeType::Article, "## Status\n\nActive\n", None);
        assert_eq!(
            restored_project_status(&n),
            (Some("## Status\n\nActive\n".into()), None)
        );
    }

    #[test]
    fn the_preview_warns_when_a_backup_has_no_activity_history() {
        let mut counts = EntityCounts {
            nodes: 3,
            edges: 0,
            tags: 0,
            notes: 0,
            tasks: 0,
            attachments: 0,
            people: 0,
            activity: 0,
        };
        assert!(
            activity_warning(&counts).is_some_and(|w| w.contains("Activity")),
            "an archive without the log warns"
        );
        counts.activity = 12;
        assert_eq!(activity_warning(&counts), None, "one with it doesn't");
    }

    #[test]
    fn folded_pairs_and_homes_are_counted_not_named() {
        let edge = |n: u128, s: u128, t: u128, edge_type: &str| {
            serde_json::json!({
                "id": Uuid::from_u128(n), "source_id": Uuid::from_u128(s),
                "target_id": Uuid::from_u128(t), "edge_type": edge_type,
                "label": null, "created_at": "2026-01-01T00:00:00Z",
            })
        };
        let data = |edges: Vec<serde_json::Value>| -> BackupData {
            serde_json::from_value(serde_json::json!({
                "nodes": [], "edges": edges, "tags": [], "notes": [],
                "tasks": [], "attachments": [],
            }))
            .expect("backup data")
        };
        assert!(folded_links_warnings(&data(vec![edge(1, 7, 8, "related_to")])).is_empty());
        assert_eq!(
            folded_links_warnings(&data(vec![
                edge(1, 7, 8, "related_to"),
                edge(2, 8, 7, "related_to"),
                edge(3, 5, 9, "contains"),
                edge(4, 6, 9, "contains"),
                edge(5, 7, 9, "contains"),
            ])),
            vec![
                "1 related link in this backup is stored both ways; \
                 each pair is restored as one link."
                    .to_string(),
                "2 home links in this backup would give a node a second home; \
                 each node keeps its oldest home."
                    .to_string(),
            ]
        );
    }

    #[test]
    fn the_edge_rules_warning_carries_rule_names_and_counts_only() {
        use common::data_health::RuleId;
        assert_eq!(rules_warning(&[]), None, "a clean graph says nothing");
        let w = rules_warning(&[(RuleId::DependsOnArea, 2), (RuleId::ContainsCycle, 1)])
            .expect("a warning");
        assert_eq!(
            w,
            format!(
                "Some links in this backup break the data model: {} (2), {} (1). \
                 They are restored as they are, and the Data health page lists them.",
                RuleId::DependsOnArea.title(),
                RuleId::ContainsCycle.title()
            )
        );
    }

    #[test]
    fn only_this_releases_backup_format_restores() {
        let manifest = |v: u32| BackupManifest {
            schema_version: v,
            created_at: chrono::Utc::now(),
            created_by: "admin".into(),
            entity_counts: EntityCounts {
                nodes: 0,
                edges: 0,
                tags: 0,
                notes: 0,
                tasks: 0,
                attachments: 0,
                people: 0,
                activity: 0,
            },
        };
        assert!(check_format(&manifest(common::backup::SCHEMA_VERSION)).is_ok());
        let old = check_format(&manifest(2));
        assert!(
            matches!(&old, Err(ApiError::Validation(m)) if m.contains("older version") && m.contains("format 2")),
            "got {old:?}"
        );
        assert!(
            matches!(check_format(&manifest(9)), Err(ApiError::Validation(m)) if m.contains("newer"))
        );
    }
}
