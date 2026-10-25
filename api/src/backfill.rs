//! One-time data backfills that need Rust logic, run at startup after the
//! SQL migrations. Each one claims a row in `data_backfills` (migration 035)
//! inside its transaction, so it runs exactly once, even with two instances
//! starting together: the second blocks on the first's insert, then sees
//! the conflict and skips.

use common::project_status::split_from_body;
use sqlx::PgPool;
use uuid::Uuid;

/// Who the backfill's version snapshots are recorded as.
const BACKFILL_SUBJECT: &str = "system:backfill-035";

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
            record_version(&mut tx, row.id, &row.body).await?;
        }
        sqlx::query("UPDATE nodes SET body = $2, project_status = $3 WHERE id = $1")
            .bind(row.id)
            .bind(&split.body)
            .bind(&split.status)
            .execute(&mut *tx)
            .await?;
        record_version(&mut tx, row.id, &split.body).await?;
        changed += 1;
    }

    sqlx::query("UPDATE data_backfills SET rows_changed = $1 WHERE name = 'project_status_035'")
        .bind(i32::try_from(changed).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(changed)
}

async fn record_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    node_id: Uuid,
    body: &str,
) -> Result<(), sqlx::Error> {
    // `clock_timestamp()`, not the transaction's `now()`: the old and new
    // snapshots of one node must sort in the order they were taken.
    sqlx::query(
        "INSERT INTO node_versions (node_id, body, created_by, created_at) \
         VALUES ($1, $2, $3, clock_timestamp())",
    )
    .bind(node_id)
    .bind(body)
    .bind(BACKFILL_SUBJECT)
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
}
