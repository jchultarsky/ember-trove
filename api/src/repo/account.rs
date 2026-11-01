//! Account lifecycle (#239): the suspended-account list, and purging an
//! account's data.

use std::collections::HashSet;

use async_trait::async_trait;
use common::{
    EmberTroveError,
    admin::{DELETED_ACCOUNT, DELETED_ACCOUNT_NAME, PurgeCounts},
};
use sqlx::{PgConnection, PgPool};

#[async_trait]
pub trait AccountRepo: Send + Sync {
    /// Every suspended subject.
    async fn suspended(&self) -> Result<HashSet<String>, EmberTroveError>;

    /// Record the subject as suspended by `by`. Suspending an account
    /// that is already suspended keeps the first record.
    async fn suspend(&self, subject_id: &str, by: &str) -> Result<(), EmberTroveError>;

    /// Remove the subject from the list. Missing is fine. A deleted
    /// account's row stays (see `mark_deleted`).
    async fn unsuspend(&self, subject_id: &str) -> Result<(), EmberTroveError>;

    /// Keep the subject suspended after its account is deleted, until
    /// `purge_deleted_before` drops it (an ID token issued before the
    /// deletion is valid until it expires).
    async fn mark_deleted(&self, subject_id: &str, by: &str) -> Result<(), EmberTroveError>;

    /// Drop the rows of accounts deleted before `cutoff`. Returns how many.
    async fn purge_deleted_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, EmberTroveError>;

    /// What `purge` would remove now. Counts only (§14).
    async fn deletion_preview(&self, subject_id: &str) -> Result<PurgeCounts, EmberTroveError>;

    /// Delete everything the subject owns, in one transaction (owner
    /// decisions, 2026-11-01):
    /// - their nodes go, with everything on them, shared or not;
    /// - their notes go, on any node; their live tasks on other people's
    ///   nodes pass to the node's owner (off My Day), and the rest go;
    /// - their captures, People, tags, templates, saved searches,
    ///   favorites, webhooks and editor settings go;
    /// - grants to them go; what stays on other people's nodes and names
    ///   them (History entries, version authorship, `granted_by`) is
    ///   anonymised to `DELETED_ACCOUNT`, matching their sub and `email`;
    /// - their files are queued in `object_deletions`, for deletion from
    ///   storage after the commit.
    ///
    /// A second run finds nothing and returns zero counts.
    async fn purge(
        &self,
        subject_id: &str,
        email: Option<&str>,
    ) -> Result<PurgeCounts, EmberTroveError>;
}

pub struct PgAccountRepo {
    pool: PgPool,
}

impl PgAccountRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AccountRepo for PgAccountRepo {
    async fn suspended(&self) -> Result<HashSet<String>, EmberTroveError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT subject_id FROM suspended_accounts")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                EmberTroveError::Internal(format!("list suspended accounts failed: {e}"))
            })?;
        Ok(rows.into_iter().collect())
    }

    async fn suspend(&self, subject_id: &str, by: &str) -> Result<(), EmberTroveError> {
        sqlx::query(
            "INSERT INTO suspended_accounts (subject_id, suspended_by) VALUES ($1, $2)
             ON CONFLICT (subject_id) DO NOTHING",
        )
        .bind(subject_id)
        .bind(by)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("suspend account failed: {e}")))?;
        Ok(())
    }

    async fn unsuspend(&self, subject_id: &str) -> Result<(), EmberTroveError> {
        sqlx::query("DELETE FROM suspended_accounts WHERE subject_id = $1 AND deleted_at IS NULL")
            .bind(subject_id)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("unsuspend account failed: {e}")))?;
        Ok(())
    }

    async fn mark_deleted(&self, subject_id: &str, by: &str) -> Result<(), EmberTroveError> {
        sqlx::query(
            "INSERT INTO suspended_accounts (subject_id, suspended_by, deleted_at)
             VALUES ($1, $2, now())
             ON CONFLICT (subject_id) DO UPDATE
                SET deleted_at = COALESCE(suspended_accounts.deleted_at, now())",
        )
        .bind(subject_id)
        .bind(by)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("mark account deleted failed: {e}")))?;
        Ok(())
    }

    async fn purge_deleted_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, EmberTroveError> {
        let done = sqlx::query("DELETE FROM suspended_accounts WHERE deleted_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                EmberTroveError::Internal(format!("purge deleted accounts failed: {e}"))
            })?;
        Ok(done.rows_affected())
    }

    async fn deletion_preview(&self, subject_id: &str) -> Result<PurgeCounts, EmberTroveError> {
        let mut conn = self.pool.acquire().await.map_err(purge_err)?;
        let shared_nodes = count(&mut conn, SHARED_NODES, subject_id).await?;
        let handed_over = count(&mut conn, TASKS_HANDED_OVER, subject_id).await?;
        let all_tasks = count(
            &mut conn,
            "SELECT count(*) FROM node_tasks WHERE owner_id = $1",
            subject_id,
        )
        .await?;
        let mut settings = 0;
        for (table, column) in SETTINGS {
            let sql = format!("SELECT count(*) FROM {table} WHERE {column} = $1");
            settings += count(&mut conn, &sql, subject_id).await?;
        }
        Ok(PurgeCounts {
            nodes: count(
                &mut conn,
                "SELECT count(*) FROM nodes WHERE owner_id = $1",
                subject_id,
            )
            .await?,
            shared_nodes,
            notes: count(
                &mut conn,
                "SELECT count(*) FROM node_notes WHERE owner_id = $1",
                subject_id,
            )
            .await?,
            tasks: all_tasks.saturating_sub(handed_over),
            tasks_handed_over: handed_over,
            captures: count(
                &mut conn,
                "SELECT count(*) FROM captures WHERE owner_id = $1",
                subject_id,
            )
            .await?,
            people: count(
                &mut conn,
                "SELECT count(*) FROM people WHERE owner_id = $1",
                subject_id,
            )
            .await?,
            files: count(&mut conn, FILES, subject_id).await?,
            settings,
        })
    }

    async fn purge(
        &self,
        subject_id: &str,
        email: Option<&str>,
    ) -> Result<PurgeCounts, EmberTroveError> {
        let mut tx = self.pool.begin().await.map_err(purge_err)?;
        // Two purges of one account (a double click) queue behind each
        // other; the second then finds nothing.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ember.purge.' || $1))")
            .bind(subject_id)
            .execute(&mut *tx)
            .await
            .map_err(purge_err)?;

        let shared_nodes = count(&mut tx, SHARED_NODES, subject_id).await?;

        // Files first, while the attachment rows still say where they are.
        let files = exec(
            &mut tx,
            "INSERT INTO object_deletions (s3_key)
             SELECT a.s3_key FROM attachments a JOIN nodes n ON n.id = a.node_id
             WHERE n.owner_id = $1
             ON CONFLICT (s3_key) DO NOTHING",
            subject_id,
        )
        .await?;

        // The edge archives have no foreign key, so the node cascade
        // misses them.
        for table in EDGE_ARCHIVES {
            let sql = format!(
                "DELETE FROM {table} WHERE source_id IN (SELECT id FROM nodes WHERE owner_id = $1)
                    OR target_id IN (SELECT id FROM nodes WHERE owner_id = $1)"
            );
            exec(&mut tx, &sql, subject_id).await?;
        }

        // Live tasks on other people's nodes pass to the node's owner, off
        // My Day (it was the deleted account's day, not theirs).
        let tasks_handed_over = exec(
            &mut tx,
            "UPDATE node_tasks t
             SET owner_id = n.owner_id, focus_date = NULL, sort_order = 0
             FROM nodes n
             WHERE t.node_id = n.id AND t.owner_id = $1 AND n.owner_id <> $1
               AND t.deleted_at IS NULL",
            subject_id,
        )
        .await?;
        let tasks = exec(
            &mut tx,
            "DELETE FROM node_tasks WHERE owner_id = $1",
            subject_id,
        )
        .await?;
        let notes = exec(
            &mut tx,
            "DELETE FROM node_notes WHERE owner_id = $1",
            subject_id,
        )
        .await?;
        let captures = exec(
            &mut tx,
            "DELETE FROM captures WHERE owner_id = $1",
            subject_id,
        )
        .await?;
        let people = exec(
            &mut tx,
            "DELETE FROM people WHERE owner_id = $1",
            subject_id,
        )
        .await?;
        // Before the nodes, so favorites of their own nodes are counted
        // here rather than vanishing in the cascade.
        let mut settings = 0;
        for (table, column) in SETTINGS {
            let sql = format!("DELETE FROM {table} WHERE {column} = $1");
            settings += exec(&mut tx, &sql, subject_id).await?;
        }

        exec(
            &mut tx,
            "DELETE FROM permissions WHERE subject_id = $1",
            subject_id,
        )
        .await?;
        anonymise(&mut tx, subject_id, email).await?;

        let nodes = exec(&mut tx, "DELETE FROM nodes WHERE owner_id = $1", subject_id).await?;

        tx.commit().await.map_err(purge_err)?;
        Ok(PurgeCounts {
            nodes,
            shared_nodes,
            notes,
            tasks,
            tasks_handed_over,
            captures,
            people,
            files,
            settings,
        })
    }
}

/// Their nodes that someone else holds a grant on.
const SHARED_NODES: &str = "SELECT count(DISTINCT n.id) FROM nodes n
     JOIN permissions p ON p.node_id = n.id AND p.subject_id <> $1
     WHERE n.owner_id = $1";

/// Their live tasks on other people's nodes.
const TASKS_HANDED_OVER: &str = "SELECT count(*) FROM node_tasks t JOIN nodes n ON n.id = t.node_id
     WHERE t.owner_id = $1 AND n.owner_id <> $1 AND t.deleted_at IS NULL";

/// Files attached to their nodes.
const FILES: &str = "SELECT count(*) FROM attachments a JOIN nodes n ON n.id = a.node_id
     WHERE n.owner_id = $1";

/// Per-account settings: table and the column naming the account. Fixed
/// identifiers only; the subject is always a bind parameter.
const SETTINGS: [(&str, &str); 6] = [
    ("tags", "owner_id"),
    ("node_templates", "created_by"),
    ("search_presets", "owner_id"),
    ("user_favorites", "owner_id"),
    ("webhooks", "owner_id"),
    ("editor_prefs", "owner_id"),
];

/// Archives of removed edges (migrations 033, 034, 041, 042).
const EDGE_ARCHIVES: [&str; 4] = [
    "edges_removed_033",
    "edges_removed_034",
    "edges_removed_041",
    "edges_removed_042",
];

/// Replace the subject (and their email) on what stays behind: History
/// entries on other people's nodes, the authorship of versions they saved
/// there, grants they made, suspensions they made and backups they took.
/// Entries they wrote lose their email and show `DELETED_ACCOUNT_NAME`;
/// any metadata value that is their sub or their email (a grant or an
/// invite naming them) is replaced too.
async fn anonymise(
    conn: &mut PgConnection,
    subject_id: &str,
    email: Option<&str>,
) -> Result<(), EmberTroveError> {
    for (table, column) in [
        ("permissions", "granted_by"),
        ("node_versions", "created_by"),
        ("suspended_accounts", "suspended_by"),
        ("backup_jobs", "created_by"),
    ] {
        let sql = format!("UPDATE {table} SET {column} = $2 WHERE {column} = $1");
        sqlx::query(&sql)
            .bind(subject_id)
            .bind(DELETED_ACCOUNT)
            .execute(&mut *conn)
            .await
            .map_err(purge_err)?;
    }
    sqlx::query(
        "UPDATE activity_log a SET
            subject_id = CASE WHEN a.subject_id = $1 THEN $3 ELSE a.subject_id END,
            metadata = CASE WHEN jsonb_typeof(a.metadata) <> 'object' THEN a.metadata
                ELSE COALESCE((
                    SELECT jsonb_object_agg(e.key, CASE
                        WHEN a.subject_id = $1 AND e.key = 'actor_name' THEN to_jsonb($4::text)
                        WHEN e.value = to_jsonb($1::text) THEN to_jsonb($3::text)
                        WHEN $2::text IS NOT NULL AND jsonb_typeof(e.value) = 'string'
                             AND lower(e.value #>> '{}') = lower($2::text)
                            THEN to_jsonb($4::text)
                        ELSE e.value END)
                    FROM jsonb_each(a.metadata) e
                    WHERE NOT (a.subject_id = $1 AND e.key = 'actor_email')
                ), '{}'::jsonb) END
         WHERE a.subject_id = $1
            OR (jsonb_typeof(a.metadata) = 'object' AND EXISTS (
                SELECT 1 FROM jsonb_each(a.metadata) e
                WHERE e.value = to_jsonb($1::text)
                   OR ($2::text IS NOT NULL AND jsonb_typeof(e.value) = 'string'
                       AND lower(e.value #>> '{}') = lower($2::text))
            ))",
    )
    .bind(subject_id)
    .bind(email)
    .bind(DELETED_ACCOUNT)
    .bind(DELETED_ACCOUNT_NAME)
    .execute(&mut *conn)
    .await
    .map_err(purge_err)?;
    Ok(())
}

async fn count(
    conn: &mut PgConnection,
    sql: &str,
    subject_id: &str,
) -> Result<u64, EmberTroveError> {
    let n: i64 = sqlx::query_scalar(sql)
        .bind(subject_id)
        .fetch_one(&mut *conn)
        .await
        .map_err(purge_err)?;
    Ok(u64::try_from(n).unwrap_or(0))
}

async fn exec(
    conn: &mut PgConnection,
    sql: &str,
    subject_id: &str,
) -> Result<u64, EmberTroveError> {
    let done = sqlx::query(sql)
        .bind(subject_id)
        .execute(&mut *conn)
        .await
        .map_err(purge_err)?;
    Ok(done.rows_affected())
}

/// No subject or content in the message: it may reach the logs.
fn purge_err(e: sqlx::Error) -> EmberTroveError {
    EmberTroveError::Internal(format!("account purge failed: {e}"))
}
