//! One-time data backfills that need Rust logic, run at startup after the
//! SQL migrations. Each one claims a row in `data_backfills` (migration 035)
//! inside its transaction, so it runs exactly once, even with two instances
//! starting together: the second blocks on the first's insert, then sees
//! the conflict and skips.

use common::brief::{StatusSection, status_section};
use common::project_status::split_from_body;
use sqlx::PgPool;
use uuid::Uuid;

/// Who the 035 backfill's version snapshots are recorded as.
const BACKFILL_SUBJECT: &str = "system:backfill-035";
/// Who the Status-into-Brief backfill's version snapshots are recorded as.
const STATUS_INTO_BRIEF_SUBJECT: &str = "system:backfill-status-into-brief";

/// #143: move each project's legacy status token out of its body into
/// `nodes.project_status`, using `split_from_body` (the same rule project
/// creation applies to a template's body). Returns how many nodes changed;
/// 0 if it already ran.
///
/// It rewrites user content, so for each node, in one transaction:
/// - the old body goes into `node_versions`, unless it's already the latest
///   snapshot, and so does the new one. The Versions panel can restore it.
/// - `updated_at` is left alone (`ember.keep_updated_at`).
/// - only nodes with no `project_status` yet and a parsable token change;
///   anything else is left exactly as it was.
///
/// Logs counts only, never content.
pub async fn project_status_035(pool: &PgPool) -> Result<u64, sqlx::Error> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        body: String,
    }

    let mut tx = pool.begin().await?;
    let claimed = sqlx::query(
        "INSERT INTO data_backfills (name) VALUES ('project_status_035') ON CONFLICT DO NOTHING",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if !claimed {
        tx.rollback().await?;
        return Ok(0);
    }
    sqlx::query("SET LOCAL ember.keep_updated_at = 'on'")
        .execute(&mut *tx)
        .await?;

    // A coarse SQL prefilter (a heading that mentions "status"); the Rust
    // parser makes the real decision.
    let rows = sqlx::query_as::<_, Row>(
        r#"
        SELECT id, body FROM nodes
        WHERE node_type = 'project'::node_type
          AND project_status IS NULL
          AND body ~* '(^|\n)[ \t]*#{1,6}[ \t]+[^\n]*status'
        FOR UPDATE
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;

    let mut changed: u64 = 0;
    for row in rows {
        let Some(split) = split_from_body(&row.body) else {
            continue;
        };
        let latest: Option<String> = sqlx::query_scalar(
            "SELECT body FROM node_versions WHERE node_id = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(row.id)
        .fetch_optional(&mut *tx)
        .await?;
        if latest.as_deref() != Some(row.body.as_str()) {
            record_version(&mut tx, row.id, &row.body, BACKFILL_SUBJECT).await?;
        }
        sqlx::query("UPDATE nodes SET body = $2, project_status = $3 WHERE id = $1")
            .bind(row.id)
            .bind(&split.body)
            .bind(&split.status)
            .execute(&mut *tx)
            .await?;
        record_version(&mut tx, row.id, &split.body, BACKFILL_SUBJECT).await?;
        changed += 1;
    }

    sqlx::query("UPDATE data_backfills SET rows_changed = $1 WHERE name = 'project_status_035'")
        .bind(i32::try_from(changed).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(changed)
}

/// What the Status-into-Brief backfill did. Counts only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusIntoBrief {
    /// Projects whose Status section became their Brief.
    pub moved: u64,
    /// Projects whose empty Status heading was removed (no Brief made).
    pub emptied: u64,
    /// Projects left as they were: the section is more than a Brief holds.
    pub too_large: u64,
}

/// Owner decision, 2026-11-11: every project that has a Status section in
/// its body and no Brief gets that section as its Brief, and the section
/// leaves the body (one fact, one home, #282 §6). The Brief replaces the
/// Status section; the owner then drops it from the project template.
///
/// The rule is `common::brief::status_section`, the one the editor's "Move
/// it into the brief" uses, so the two can't disagree:
/// - only a heading that reads exactly "Status" or "Project Status";
/// - the whole section must fit a Brief. One that doesn't is left exactly
///   as it was (`too_large`): nothing is ever cut;
/// - a heading with nothing under it is removed, and no Brief is made.
///
/// It rewrites user content, so for each project, in one transaction:
/// - the old body goes into `node_versions` (unless it's already the
///   latest snapshot), and so does the new one. Version History restores
///   it;
/// - the node's `updated_at` is left alone (`ember.keep_updated_at`);
/// - the Brief is dated by the node's `updated_at`, the last time those
///   words were saved, not by today: a status written months ago must not
///   read "updated just now", and it gets the stale marker it deserves;
/// - its author is the project's owner. Who wrote the words in the body is
///   not recorded;
/// - a project that has a Brief is never touched, whatever its body says.
///
/// Runs once (`data_backfills`), like [`project_status_035`]. Logs counts
/// only, never content.
pub async fn status_into_brief(pool: &PgPool) -> Result<StatusIntoBrief, sqlx::Error> {
    const NAME: &str = "status_into_brief";
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        owner_id: String,
        body: String,
        updated_at: chrono::DateTime<chrono::Utc>,
    }

    let mut done = StatusIntoBrief::default();
    let mut tx = pool.begin().await?;
    let claimed =
        sqlx::query("INSERT INTO data_backfills (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(NAME)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            == 1;
    if !claimed {
        tx.rollback().await?;
        return Ok(done);
    }
    sqlx::query("SET LOCAL ember.keep_updated_at = 'on'")
        .execute(&mut *tx)
        .await?;

    // A coarse SQL prefilter (a heading that mentions "status"); the Rust
    // parser makes the real decision.
    let rows = sqlx::query_as::<_, Row>(
        r#"
        SELECT n.id, n.owner_id, n.body, n.updated_at FROM nodes n
        WHERE n.node_type = 'project'::node_type
          AND n.body ~* '(^|\n)[ \t]*#{1,6}[ \t]+[^\n]*status'
          AND NOT EXISTS (SELECT 1 FROM project_briefs b WHERE b.node_id = n.id)
        FOR UPDATE OF n
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;

    for row in rows {
        let new_body = match status_section(&row.body) {
            StatusSection::Absent => continue,
            StatusSection::TooLarge => {
                done.too_large += 1;
                continue;
            }
            StatusSection::Empty { body } => {
                done.emptied += 1;
                body
            }
            StatusSection::Fits(moved) => {
                // The Brief first: with none made, the section must stay.
                let made = sqlx::query(
                    "INSERT INTO project_briefs \
                         (node_id, headline, pitch, points, author_id, stated_at, updated_at) \
                     VALUES ($1, NULL, $2, $3, $4, $5, $5) \
                     ON CONFLICT (node_id) DO NOTHING",
                )
                .bind(row.id)
                .bind(&moved.fields.pitch)
                .bind(&moved.fields.points)
                .bind(&row.owner_id)
                .bind(row.updated_at)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                    == 1;
                if !made {
                    continue;
                }
                done.moved += 1;
                moved.body
            }
        };
        let latest: Option<String> = sqlx::query_scalar(
            "SELECT body FROM node_versions WHERE node_id = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(row.id)
        .fetch_optional(&mut *tx)
        .await?;
        if latest.as_deref() != Some(row.body.as_str()) {
            record_version(&mut tx, row.id, &row.body, STATUS_INTO_BRIEF_SUBJECT).await?;
        }
        sqlx::query("UPDATE nodes SET body = $2 WHERE id = $1")
            .bind(row.id)
            .bind(&new_body)
            .execute(&mut *tx)
            .await?;
        record_version(&mut tx, row.id, &new_body, STATUS_INTO_BRIEF_SUBJECT).await?;
    }

    sqlx::query("UPDATE data_backfills SET rows_changed = $2 WHERE name = $1")
        .bind(NAME)
        .bind(i32::try_from(done.moved + done.emptied).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(done)
}

async fn record_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
    body: &str,
    subject: &str,
) -> Result<(), sqlx::Error> {
    // `clock_timestamp()`, not the transaction's `now()`: the old and new
    // snapshots of one node must sort in the order they were taken.
    sqlx::query(
        "INSERT INTO node_versions (node_id, body, created_by, created_at) \
         VALUES ($1, $2, $3, clock_timestamp())",
    )
    .bind(node_id)
    .bind(body)
    .bind(subject)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Run every pending backfill. Failures are logged, not fatal: the
/// transaction rolls back, nothing is claimed, and the next start retries.
pub async fn run_all(pool: &PgPool) {
    match project_status_035(pool).await {
        Ok(0) => {}
        Ok(n) => tracing::info!("backfill project_status_035: {n} project(s) updated"),
        Err(e) => tracing::warn!("backfill project_status_035 failed (will retry): {e}"),
    }
    match status_into_brief(pool).await {
        Ok(done) if done == StatusIntoBrief::default() => {}
        Ok(done) => tracing::info!(
            "backfill status_into_brief: {} project(s) got their Status section as a Brief, \
             {} empty Status heading(s) removed, {} left alone (more than a Brief holds)",
            done.moved,
            done.emptied,
            done.too_large
        ),
        Err(e) => tracing::warn!("backfill status_into_brief failed (will retry): {e}"),
    }
}
