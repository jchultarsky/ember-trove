//! Backup and restore service.
//!
//! Archives are stored in S3 under `backups/<job-id>.tar.gz`.
//! Each archive contains:
//! - `manifest.json` — metadata and entity counts
//! - `data.json`     — all serialised entities
//! - `attachments/<attachment-uuid>` — raw attachment bytes (format 5; formats
//!   3 and 4 used `attachments/<node-uuid>/<filename>`, which two attachments
//!   can share, and restore still reads them: `match_attachment_entries`)

use bytes::Bytes;
use common::{
    activity::ActivityRecord,
    attachment::Attachment,
    backup::{BackupData, BackupJob, BackupManifest, BackupNote, BackupPreview, EntityCounts},
    id::NodeId,
};
use flate2::{Compression, write::GzEncoder};
use std::collections::HashSet;
use std::io::Write;
use tar::Builder;
use uuid::Uuid;

use crate::{
    error::ApiError,
    object_store::ObjectStore,
    repo::{self, backup::BackupRepo, object_deletion::ObjectDeletionRepo},
    state::AppState,
};

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

/// The counts the preview shows, as the restore will land them: a format-3
/// note without a node comes back as a capture (#234), so it counts there
/// and not as a note. A no-op for format 4, whose notes all have a node.
fn counts_as_restored(mut counts: EntityCounts, notes: &[BackupNote]) -> EntityCounts {
    let moved = notes
        .iter()
        .filter(|n| common::capture::from_nodeless_note(n).is_some())
        .count();
    let moved = u32::try_from(moved).unwrap_or(u32::MAX);
    counts.notes = counts.notes.saturating_sub(moved);
    counts.captures = counts.captures.saturating_add(moved);
    counts
}

/// Only archives this release can read restore (#222): its own format,
/// format 4, whose attachment files sit under their node and file name
/// (#281), and format 3, whose captures are also notes without a node
/// (#234). Checked before
/// anything else, so an unreadable archive costs no pre-restore snapshot
/// and never reaches the parser, which would fail on its data with a 500.
fn check_format(manifest: &BackupManifest) -> Result<(), ApiError> {
    let (oldest, wanted) = (
        common::backup::OLDEST_RESTORABLE,
        common::backup::SCHEMA_VERSION,
    );
    let range = format!("format {oldest} to {wanted}");
    match manifest.schema_version {
        v if (oldest..=wanted).contains(&v) => Ok(()),
        v if v < oldest => Err(ApiError::Validation(format!(
            "This backup was made by an older version of Ember Trove (format {v}). \
             This release restores {range} only."
        ))),
        v => Err(ApiError::Validation(format!(
            "This backup was made by a newer version of Ember Trove (format {v}). \
             This release restores {range} only."
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

/// The restore preview's note on webhooks that come back switched off
/// (`BackupWebhook::restored_active`). A count only.
fn webhooks_warning(switched_off: usize) -> Option<String> {
    (switched_off > 0).then(|| {
        let (s, they, are, their) = if switched_off == 1 {
            ("", "it", "is", "its")
        } else {
            ("s", "they", "are", "their")
        };
        format!(
            "{switched_off} webhook{s} in this backup signed {their} payloads. Signing secrets \
             are never backed up, so {they} {are} restored switched off until {their} owner \
             enters a secret again."
        )
    })
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
        // No path in the message: an attachment's holds its file name.
        .map_err(|e| ApiError::Internal(format!("tar append failed: {e}")))?;
    Ok(())
}

// ── create_backup ──────────────────────────────────────────────────────────────

/// Opens the transaction a backup reads in: `REPEATABLE READ`, so every
/// table is read as of one instant, and `READ ONLY`. Read in separate
/// queries, a node created or a tag deleted between two of them left rows
/// pointing at nothing, and restoring that archive failed on its foreign
/// keys (#281). The snapshot is taken by the first query run on it.
pub(crate) async fn begin_snapshot(
    pool: &sqlx::PgPool,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, ApiError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ApiError::Internal(format!("begin backup snapshot failed: {e}")))?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(format!("set backup snapshot isolation failed: {e}")))?;
    Ok(tx)
}

/// Everything an archive's `data.json` holds, all owners (the backup is
/// admin-run), read on the caller's connection: [`begin_snapshot`]'s, so
/// the lists agree with each other.
pub(crate) async fn read_snapshot(conn: &mut sqlx::PgConnection) -> Result<BackupData, ApiError> {
    let nodes = repo::node::list_all_on(&mut *conn).await?;
    let tags = repo::tag::list_all_on(&mut *conn).await?;
    let notes: Vec<BackupNote> = repo::note::list_all_on(&mut *conn)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    // Captures (#234).
    let captures = repo::capture::list_all_on(&mut *conn).await?;
    let tasks = repo::task::list_all_on(&mut *conn).await?;
    let recurrence_links = repo::task::recurrence_links_on(&mut *conn).await?;
    // Project Briefs (#282), all owners, in the same snapshot as their nodes.
    let briefs = repo::brief::list_all_on(&mut *conn).await?;
    let edges = repo::edge::list_all_on(&mut *conn).await?;
    let attachments = repo::attachment::list_all_on(&mut *conn).await?;
    let node_links = repo::node_link::list_all_on(&mut *conn).await?;
    let favorites = repo::favorite::list_all_on(&mut *conn).await?;
    let permissions = repo::permission::list_all_on(&mut *conn, None).await?;
    let node_versions = repo::node_version::list_all_on(&mut *conn).await?;
    let node_positions = repo::graph::list_positions_on(&mut *conn, None).await?;
    // People (#147): third-party contact details.
    let (people, node_people) = repo::person::list_all_on(&mut *conn).await?;
    // Activity (#208): the History record (ONTOLOGY §10).
    let activity = repo::activity::list_all_on(&mut *conn).await?;
    // Format 5 (#281): what a restore onto an empty database used to lose.
    let node_templates = repo::template::list_on(&mut *conn, None).await?;
    let search_presets = repo::search_presets::list_all_on(&mut *conn).await?;
    let editor_prefs = repo::editor_pref::list_all_on(&mut *conn).await?;
    // Without their signing secrets: the query never reads them.
    let webhooks = repo::webhook::list_all_for_backup(&mut *conn).await?;

    Ok(BackupData {
        nodes,
        edges,
        tags,
        notes,
        tasks,
        attachments,
        node_links,
        favorites,
        permissions,
        node_versions,
        node_positions,
        people,
        node_people,
        activity,
        captures,
        node_templates: Some(node_templates),
        search_presets: Some(search_presets),
        editor_prefs: Some(editor_prefs),
        webhooks: Some(webhooks),
        recurrence_links,
        briefs,
    })
}

/// Packs a manifest, the data and every attachment's bytes into a tar.gz,
/// in memory. A file the store can't return is skipped with a warning; its
/// row is still in `data`.
async fn build_archive(
    manifest: &BackupManifest,
    data: &BackupData,
    store: &dyn ObjectStore,
) -> Result<Vec<u8>, ApiError> {
    let manifest_bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| ApiError::Internal(format!("manifest serialise failed: {e}")))?;
    let data_bytes = serde_json::to_vec_pretty(data)
        .map_err(|e| ApiError::Internal(format!("data serialise failed: {e}")))?;

    let gz_buf: Vec<u8> = Vec::new();
    let enc = GzEncoder::new(gz_buf, Compression::default());
    let mut builder = Builder::new(enc);

    add_bytes_to_archive(&mut builder, "manifest.json", &manifest_bytes)?;
    add_bytes_to_archive(&mut builder, "data.json", &data_bytes)?;

    // Each attachment under its own id (#281): two on one node can share a
    // file name, and the old node-and-name path made them one entry name.
    for att in &data.attachments {
        match store.get(&att.s3_key).await {
            Ok(file_bytes) => {
                let archive_path = common::backup::attachment_entry_path(att.id);
                add_bytes_to_archive(&mut builder, &archive_path, &file_bytes)?;
            }
            Err(e) => {
                // The attachment's id, never its key: the key holds the file
                // name, and this command's output can reach the release log.
                tracing::warn!(
                    attachment_id = %att.id,
                    "attachment fetch failed during backup, skipping: {e}"
                );
            }
        }
    }

    let enc = builder
        .into_inner()
        .map_err(|e| ApiError::Internal(format!("tar finish failed: {e}")))?;
    enc.finish()
        .map_err(|e| ApiError::Internal(format!("gzip finish failed: {e}")))
}

/// Collect all owner data, pack it into a tar.gz archive, upload to S3,
/// and record the job in `backup_jobs`.
pub async fn create_backup(
    state: &AppState,
    owner_id: &str,
    comment: Option<&str>,
) -> Result<BackupJob, ApiError> {
    // Every table as of one instant (#281). The transaction only reads, so
    // there is nothing to commit, and it ends before the files are fetched:
    // a snapshot shouldn't stay open across slow storage calls.
    let mut tx = begin_snapshot(&state.pool).await?;
    let data = read_snapshot(&mut tx).await?;
    tx.rollback()
        .await
        .map_err(|e| ApiError::Internal(format!("end backup snapshot failed: {e}")))?;

    let entity_counts = EntityCounts {
        nodes: data.nodes.len() as u32,
        edges: data.edges.len() as u32,
        tags: data.tags.len() as u32,
        notes: data.notes.len() as u32,
        tasks: data.tasks.len() as u32,
        attachments: data.attachments.len() as u32,
        people: data.people.len() as u32,
        activity: data.activity.len() as u32,
        captures: data.captures.len() as u32,
    };

    let job_id = Uuid::new_v4();
    let manifest = BackupManifest {
        schema_version: common::backup::SCHEMA_VERSION,
        created_at: chrono::Utc::now(),
        created_by: owner_id.to_string(),
        entity_counts: entity_counts.clone(),
    };

    let gz_bytes = build_archive(&manifest, &data, state.object_store.as_ref()).await?;

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

// ── delete and prune ───────────────────────────────────────────────────────────

/// Delete one backup: its archive, then its row. An archive holds everyone's
/// data, so it must never outlive its row unnoticed: if the store refuses,
/// the key goes to the `object_deletions` outbox (the daily drain retries
/// it), and if even that fails, the row stays so the backup is still listed.
pub async fn delete_backup(
    backups: &dyn BackupRepo,
    store: &dyn ObjectStore,
    outbox: &dyn ObjectDeletionRepo,
    id: Uuid,
) -> Result<(), ApiError> {
    let job = backups.get(id).await.map_err(ApiError::from)?;
    if let Err(e) = store.delete(&job.s3_key).await {
        tracing::warn!(%id, "backup archive delete failed, queued for retry: {e}");
        outbox.enqueue(&job.s3_key).await.map_err(ApiError::from)?;
    }
    backups.delete(id).await.map_err(ApiError::from)
}

/// Delete the automatic backups past the newest `keep`
/// (`common::backup::automatic_to_prune`); an admin's are never touched.
/// One already gone (an admin deleted it meanwhile) counts as deleted.
/// Returns how many were deleted.
pub async fn prune_automatic(
    backups: &dyn BackupRepo,
    store: &dyn ObjectStore,
    outbox: &dyn ObjectDeletionRepo,
    keep: usize,
) -> Result<usize, ApiError> {
    let jobs = backups.list_all().await.map_err(ApiError::from)?;
    let doomed = common::backup::automatic_to_prune(&jobs, keep);
    for id in &doomed {
        match delete_backup(backups, store, outbox, *id).await {
            Ok(()) | Err(ApiError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(doomed.len())
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
    let mut entity_counts = manifest.entity_counts;

    // Warn about current data that will be replaced.
    let current_nodes = state.nodes.list_all().await.map_err(ApiError::from)?;
    if !current_nodes.is_empty() {
        warnings.push(format!(
            "{} existing node(s) and all associated data will be deleted before restore.",
            current_nodes.len()
        ));
    }

    if let Some(w) = activity_warning(&entity_counts) {
        warnings.push(w);
    }
    // Restore never refuses old data (owner decision, #224); it says what
    // the archive's links break, in counts only.
    // An unreadable data.json fails the restore itself later; the preview
    // still shows, saying it couldn't check.
    match extract_data(&archive_bytes) {
        Ok(data) => {
            entity_counts = counts_as_restored(entity_counts, &data.notes);
            warnings.extend(edge_rules_warning(&data));
            warnings.extend(folded_links_warnings(&data));
            warnings.extend(webhooks_warning(data.webhooks_switched_off()));
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
        entity_counts,
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
    upload_restored_files(
        state.object_store.as_ref(),
        &data.attachments,
        &attachment_files,
    )
    .await;

    Ok(())
}

/// Puts an archive's attachment files back under their rows' keys. Each
/// file goes to the attachment `match_attachment_entries` names for it and
/// to no other: restore used to take the first attachment with the same
/// node and file name, so one file's bytes could land under another's key
/// (#281). Best effort: a failed upload is logged, not fatal. Returns how
/// many files were uploaded.
async fn upload_restored_files(
    store: &dyn ObjectStore,
    attachments: &[Attachment],
    files: &[(String, Vec<u8>)],
) -> usize {
    let entries: Vec<(&str, u64)> = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.len() as u64))
        .collect();
    let matched = common::backup::match_attachment_entries(attachments, &entries);

    let (mut uploaded, mut unmatched) = (0usize, 0usize);
    for ((_, bytes), index) in files.iter().zip(matched) {
        let Some(att) = index.and_then(|i| attachments.get(i)) else {
            unmatched += 1;
            continue;
        };
        match store
            .put(
                &att.s3_key,
                Bytes::copy_from_slice(bytes),
                &att.content_type,
            )
            .await
        {
            Ok(()) => uploaded += 1,
            // The attachment's id, never its key: the key holds the file name.
            Err(e) => tracing::warn!(
                attachment_id = %att.id,
                "re-upload attachment to S3 failed (non-fatal): {e}"
            ),
        }
    }
    if unmatched > 0 {
        tracing::warn!(
            unmatched,
            "restore: archive files that can't be told apart were not uploaded"
        );
    }
    uploaded
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
    // The attachment trigger queues every live file for deletion (#239);
    // the archive's own come back under their old keys and leave the queue
    // again at the end.
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

    // What no node holds, which the cascade above never reached: tasks
    // without a Project, the captures (#234) and the people registry (#281:
    // memberships went with the nodes, the people themselves did not). A
    // restore used to leave them, so the live database's own survived next
    // to the archive's.
    for (what, sql) in [
        (
            "nodeless tasks",
            "DELETE FROM node_tasks WHERE node_id IS NULL",
        ),
        ("captures", "DELETE FROM captures"),
        ("people", "DELETE FROM people"),
    ] {
        sqlx::query(sql)
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("delete {what} failed: {e}")))?;
    }

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

    // Captures (#234): the archive's own, plus, from a format-3 archive,
    // its notes without a node (`capture::from_nodeless_note`, 043's rule).
    let captures = data.captures.iter().cloned().chain(
        data.notes
            .iter()
            .filter_map(common::capture::from_nodeless_note),
    );
    for capture in captures {
        sqlx::query(
            "INSERT INTO captures (id, owner_id, body, color, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (id) DO NOTHING",
        )
        .bind(capture.id.0)
        .bind(&capture.owner_id)
        .bind(&capture.body)
        .bind(capture.color.as_str())
        .bind(capture.created_at)
        .bind(capture.updated_at)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("insert capture failed: {e}")))?;
    }

    // Insert notes (each on its node; the nodeless ones became captures).
    for note in &data.notes {
        let Some(node_id) = note.node_id else {
            continue;
        };
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
        .bind(node_id.0)
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

    // Which task scheduled which occurrence (#281), once every task is in:
    // the link is a foreign key to another task. A link whose parent isn't
    // in the archive is dropped; that only disables the retraction for it.
    if !data.recurrence_links.is_empty() {
        let tasks: Vec<Uuid> = data.recurrence_links.iter().map(|l| l.task_id.0).collect();
        let parents: Vec<Uuid> = data
            .recurrence_links
            .iter()
            .map(|l| l.parent_id.0)
            .collect();
        sqlx::query(
            r#"
            UPDATE node_tasks t
            SET recurrence_parent_id = l.parent
            FROM UNNEST($1::uuid[], $2::uuid[]) AS l(task, parent)
            WHERE t.id = l.task
              AND EXISTS (SELECT 1 FROM node_tasks p WHERE p.id = l.parent)
            "#,
        )
        .bind(&tasks)
        .bind(&parents)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("restore recurrence links failed: {e}")))?;
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

    restore_account_settings(conn, data).await?;

    // Activity last (#208): every row needs its node. Deleting the nodes
    // above cascaded the old log away, so this brings History back.
    let restored_nodes: HashSet<Uuid> = data.nodes.iter().map(|n| n.id.0).collect();
    restore_activity(conn, &data.activity, &restored_nodes).await?;

    // Project Briefs (#282). A Brief whose node isn't in the archive is
    // skipped rather than failing the FK. A backup reads one snapshot now
    // (#281), so that can only be a hand-built or damaged archive.
    for b in data
        .briefs
        .iter()
        .filter(|b| restored_nodes.contains(&b.node_id.0))
    {
        sqlx::query(
            r#"
            INSERT INTO project_briefs
                (node_id, headline, pitch, points, author_id, stated_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (node_id) DO UPDATE
                SET headline = EXCLUDED.headline, pitch = EXCLUDED.pitch,
                    points = EXCLUDED.points, author_id = EXCLUDED.author_id,
                    stated_at = EXCLUDED.stated_at, updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(b.node_id.0)
        .bind(&b.headline)
        .bind(&b.pitch)
        .bind(&b.points)
        .bind(&b.author_id)
        .bind(b.stated_at)
        .bind(b.updated_at)
        .execute(&mut *conn)
        .await
        // Never include the row: it's user content.
        .map_err(|e| ApiError::Internal(format!("insert brief failed: {e}")))?;
    }

    // The archive's files are back under their old keys: keep them out of
    // the deletion queue (the drain also skips any key a row names).
    sqlx::query("DELETE FROM object_deletions d USING attachments a WHERE a.s3_key = d.s3_key")
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::Internal(format!("unqueue restored files failed: {e}")))?;

    Ok(())
}

/// Replaces what an account keeps outside its nodes: templates, saved
/// searches, editor prefs and webhooks (format 5, #281). Each list is
/// replaced only when the archive carries it. An older archive has `None`,
/// and the live rows stay, as they did before backups held them.
async fn restore_account_settings(
    conn: &mut sqlx::PgConnection,
    data: &BackupData,
) -> Result<(), ApiError> {
    if let Some(templates) = &data.node_templates {
        sqlx::query("DELETE FROM node_templates")
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("delete templates failed: {e}")))?;
        for t in templates {
            sqlx::query(
                r#"
                INSERT INTO node_templates
                    (id, name, description, node_type, body, is_default, created_by,
                     created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(t.id.0)
            .bind(&t.name)
            .bind(&t.description)
            .bind(t.node_type.as_str())
            .bind(&t.body)
            .bind(t.is_default)
            .bind(&t.created_by)
            .bind(t.created_at)
            .bind(t.updated_at)
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("insert template failed: {e}")))?;
        }
    }

    if let Some(presets) = &data.search_presets {
        sqlx::query("DELETE FROM search_presets")
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("delete search presets failed: {e}")))?;
        for p in presets {
            sqlx::query(
                r#"
                INSERT INTO search_presets
                    (id, owner_id, name, query, fuzzy, published_only, tag_ids, tag_op,
                     created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(p.id.0)
            .bind(&p.owner_id)
            .bind(&p.name)
            .bind(&p.query)
            .bind(p.fuzzy)
            .bind(p.published_only)
            .bind(&p.tag_ids)
            .bind(&p.tag_op)
            .bind(p.created_at)
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("insert search preset failed: {e}")))?;
        }
    }

    if let Some(prefs) = &data.editor_prefs {
        sqlx::query("DELETE FROM editor_prefs")
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("delete editor prefs failed: {e}")))?;
        for p in prefs {
            sqlx::query(
                "INSERT INTO editor_prefs (owner_id, entity_kind, entity_id, height, collapsed) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&p.owner_id)
            .bind(&p.entity_kind)
            .bind(p.entity_id)
            .bind(p.height)
            .bind(p.collapsed)
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("insert editor pref failed: {e}")))?;
        }
    }

    if let Some(webhooks) = &data.webhooks {
        sqlx::query("DELETE FROM webhooks")
            .execute(&mut *conn)
            .await
            .map_err(|e| ApiError::Internal(format!("delete webhooks failed: {e}")))?;
        for w in webhooks {
            // No secret: an archive never holds one. A webhook that signed
            // comes back switched off (`BackupWebhook::restored_active`).
            sqlx::query(
                r#"
                INSERT INTO webhooks
                    (id, owner_id, url, secret, events, is_active, created_at, updated_at)
                VALUES ($1, $2, $3, NULL, $4, $5, $6, $7)
                "#,
            )
            .bind(w.id.0)
            .bind(&w.owner_id)
            .bind(&w.url)
            .bind(&w.events)
            .bind(w.restored_active())
            .bind(w.created_at)
            .bind(w.updated_at)
            .execute(&mut *conn)
            .await
            // Never include the row: its URL can carry a token.
            .map_err(|e| ApiError::Internal(format!("insert webhook failed: {e}")))?;
        }
    }
    Ok(())
}

/// Re-inserts a backup's activity rows in one statement (the log only
/// grows, so it's batched rather than one INSERT per row). Rows whose node
/// isn't being restored are skipped: until format 5 a backup read each table
/// in its own query, so a node created mid-backup could leave activity
/// behind without its node, and that row would fail the FK and abort the
/// whole restore. Actions are inserted verbatim, known to this build or not.
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
    fn a_format_3_preview_counts_nodeless_notes_as_captures() {
        let at = chrono::Utc::now();
        let note = |n: u128, node: Option<u128>| BackupNote {
            id: common::id::NoteId(Uuid::from_u128(n)),
            node_id: node.map(|m| common::id::NodeId(Uuid::from_u128(m))),
            owner_id: "alice".into(),
            body: "an idea".into(),
            color: "default".into(),
            created_at: at,
            updated_at: at,
        };
        let counts = EntityCounts {
            nodes: 1,
            edges: 0,
            tags: 0,
            notes: 3,
            tasks: 0,
            attachments: 0,
            people: 0,
            activity: 0,
            captures: 0,
        };
        let notes = [note(1, None), note(2, Some(9)), note(3, None)];
        let out = counts_as_restored(counts.clone(), &notes);
        assert_eq!((out.notes, out.captures), (1, 2));
        // Format 4: every note has a node, nothing moves.
        let out = counts_as_restored(counts, &notes[1..2]);
        assert_eq!((out.notes, out.captures), (3, 0));
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
            captures: 0,
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
                captures: 0,
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
        // The formats before the attachment paths changed still restore.
        assert!(check_format(&manifest(3)).is_ok() && check_format(&manifest(4)).is_ok());
    }

    /// An object store in memory.
    #[derive(Default)]
    struct MemStore(std::sync::Mutex<std::collections::HashMap<String, Bytes>>);

    impl MemStore {
        fn holding(files: &[(&str, &[u8])]) -> Self {
            let store = Self::default();
            for (key, bytes) in files {
                store
                    .0
                    .lock()
                    .unwrap()
                    .insert((*key).to_string(), Bytes::copy_from_slice(bytes));
            }
            store
        }

        fn at(&self, key: &str) -> Option<Bytes> {
            self.0.lock().unwrap().get(key).cloned()
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for MemStore {
        async fn put(
            &self,
            key: &str,
            data: Bytes,
            _content_type: &str,
        ) -> Result<(), common::EmberTroveError> {
            self.0.lock().unwrap().insert(key.to_string(), data);
            Ok(())
        }
        async fn get(&self, key: &str) -> Result<Bytes, common::EmberTroveError> {
            self.at(key)
                .ok_or_else(|| common::EmberTroveError::NotFound("no such object".into()))
        }
        async fn delete(&self, key: &str) -> Result<(), common::EmberTroveError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
        async fn presigned_url(
            &self,
            _key: &str,
            _expires_secs: u32,
        ) -> Result<String, common::EmberTroveError> {
            Err(common::EmberTroveError::Internal("not in tests".into()))
        }
    }

    fn manifest_at(schema_version: u32) -> BackupManifest {
        BackupManifest {
            schema_version,
            created_at: chrono::Utc::now(),
            created_by: "admin".into(),
            entity_counts: EntityCounts {
                nodes: 0,
                edges: 0,
                tags: 0,
                notes: 0,
                tasks: 0,
                attachments: 2,
                people: 0,
                activity: 0,
                captures: 0,
            },
        }
    }

    /// Two attachments on one node with the same file name, as two pasted
    /// images are, each under its own key.
    fn two_with_one_name(sizes: (i64, i64)) -> Vec<Attachment> {
        let node = Uuid::new_v4();
        [
            ("key/first/image.png", sizes.0),
            ("key/second/image.png", sizes.1),
        ]
        .into_iter()
        .map(|(key, size)| Attachment {
            id: common::id::AttachmentId(Uuid::new_v4()),
            node_id: NodeId(node),
            filename: "image.png".into(),
            content_type: "image/png".into(),
            size_bytes: size,
            s3_key: key.into(),
            created_at: chrono::Utc::now(),
        })
        .collect()
    }

    #[tokio::test]
    async fn two_files_with_one_name_come_back_under_their_own_keys() {
        // #281: restore sent both entries to the first attachment's key, so
        // that key ended up with the second file and the second key with none.
        let attachments = two_with_one_name((5, 7));
        let live = MemStore::holding(&[
            ("key/first/image.png", b"first"),
            ("key/second/image.png", b"second!"),
        ]);
        let data = BackupData {
            attachments,
            ..BackupData::default()
        };
        let archive = build_archive(&manifest_at(common::backup::SCHEMA_VERSION), &data, &live)
            .await
            .expect("archive");

        let (_, read, files) = extract_full(&archive).expect("reads back");
        assert_eq!(files.len(), 2, "one entry per attachment");
        assert_ne!(files[0].0, files[1].0, "each under its own path");
        assert!(
            files.iter().all(|(path, _)| !path.contains("image.png")),
            "no file name in an archive path"
        );

        let restored = MemStore::default();
        let uploaded = upload_restored_files(&restored, &read.attachments, &files).await;
        assert_eq!(uploaded, 2);
        assert_eq!(
            restored.at("key/first/image.png").as_deref(),
            Some(&b"first"[..])
        );
        assert_eq!(
            restored.at("key/second/image.png").as_deref(),
            Some(&b"second!"[..])
        );
    }

    #[tokio::test]
    async fn an_old_archive_with_a_shared_path_restores_each_file_once() {
        // A format-4 archive already written: both files sit under one path,
        // in the order of `attachments`, and are the same length.
        let attachments = two_with_one_name((6, 6));
        let shared = format!("attachments/{}/image.png", attachments[0].node_id.0);
        let data = BackupData {
            attachments,
            ..BackupData::default()
        };
        let mut builder = Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let manifest = serde_json::to_vec(&manifest_at(4)).unwrap();
        let json = serde_json::to_vec(&data).unwrap();
        for (path, bytes) in [
            ("manifest.json", manifest.as_slice()),
            ("data.json", json.as_slice()),
            (shared.as_str(), b"first!".as_slice()),
            (shared.as_str(), b"second".as_slice()),
        ] {
            add_bytes_to_archive(&mut builder, path, bytes).unwrap();
        }
        let archive = builder.into_inner().unwrap().finish().unwrap();

        let (_, read, files) = extract_full(&archive).expect("reads back");
        let restored = MemStore::default();
        let uploaded = upload_restored_files(&restored, &read.attachments, &files).await;
        assert_eq!(uploaded, 2);
        assert_eq!(
            restored.at("key/first/image.png").as_deref(),
            Some(&b"first!"[..])
        );
        assert_eq!(
            restored.at("key/second/image.png").as_deref(),
            Some(&b"second"[..])
        );
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_told_apart_is_not_uploaded() {
        // An old archive that lost one of two same-named, same-length files:
        // the one entry could be either, so neither key is written.
        let attachments = two_with_one_name((6, 6));
        let shared = format!("attachments/{}/image.png", attachments[0].node_id.0);
        let files = vec![(shared, b"which?".to_vec())];
        let restored = MemStore::default();
        assert_eq!(
            upload_restored_files(&restored, &attachments, &files).await,
            0
        );
        assert!(restored.0.lock().unwrap().is_empty());
    }

    #[test]
    fn the_preview_counts_webhooks_restored_switched_off() {
        assert_eq!(webhooks_warning(0), None);
        let one = webhooks_warning(1).expect("warns");
        assert!(
            one.starts_with("1 webhook in this backup signed its payloads"),
            "{one}"
        );
        assert!(one.contains("it is restored switched off"), "{one}");
        let two = webhooks_warning(2).expect("warns");
        assert!(
            two.starts_with("2 webhooks in this backup signed their payloads"),
            "{two}"
        );
        assert!(two.contains("they are restored switched off"), "{two}");
    }
}
