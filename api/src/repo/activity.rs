use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    activity::{ActivityAction, ActivityEntry, ActivityRecord, RecentActivityEntry},
    auth::AuthClaims,
    id::{ActivityId, NodeId},
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// Who an activity entry is attributed to: the subject and the actor
/// metadata (display name, email) every entry carries.
#[derive(Debug, Clone)]
pub struct ActivityActor {
    pub subject_id: String,
    pub metadata: serde_json::Value,
}

impl ActivityActor {
    #[must_use]
    pub fn from_claims(claims: &AuthClaims) -> Self {
        Self {
            subject_id: claims.sub.clone(),
            metadata: serde_json::json!({
                "actor_name": claims.name,
                "actor_email": claims.email,
            }),
        }
    }

    /// The actor metadata with `extra`'s keys merged in (`extra` wins).
    #[must_use]
    pub fn metadata_with(&self, extra: serde_json::Value) -> serde_json::Value {
        let mut meta = self.metadata.clone();
        if let (Some(obj), Some(ext)) = (meta.as_object_mut(), extra.as_object()) {
            for (k, v) in ext {
                obj.insert(k.clone(), v.clone());
            }
        }
        meta
    }
}

// `clock_timestamp()`, not the column default `now()`: `now()` is the
// transaction's start, before any row-lock wait, so two concurrent status
// changes could be stamped in the opposite order to the one they committed
// in, and History would read out of order (#208).
const INSERT_ACTIVITY: &str = r#"
    INSERT INTO activity_log (node_id, subject_id, action, metadata, created_at)
    VALUES ($1, $2, $3, $4, clock_timestamp())
"#;

/// Appends one entry on the caller's connection or transaction, so a write
/// and its History entry can commit or roll back together (#208).
pub(crate) async fn record_in(
    conn: &mut PgConnection,
    node_id: NodeId,
    subject_id: &str,
    action: ActivityAction,
    metadata: serde_json::Value,
) -> Result<(), EmberTroveError> {
    sqlx::query(INSERT_ACTIVITY)
        .bind(node_id.inner())
        .bind(subject_id)
        .bind(action.as_str())
        .bind(metadata)
        .execute(conn)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("record activity failed: {e}")))?;
    Ok(())
}

#[async_trait]
pub trait ActivityRepo: Send + Sync {
    /// Append one entry. Fire-and-forget — callers ignore errors.
    async fn record(
        &self,
        node_id: NodeId,
        subject_id: &str,
        action: ActivityAction,
        metadata: serde_json::Value,
    ) -> Result<(), EmberTroveError>;

    /// Return the most recent `limit` entries for a node (newest first).
    async fn list(
        &self,
        node_id: NodeId,
        limit: i64,
    ) -> Result<Vec<ActivityEntry>, EmberTroveError>;

    /// Return recent activity entries for nodes owned by `owner_id`,
    /// joined with each node's title.  Used by the v2.9.0 dashboard
    /// recap section ("What changed today / yesterday").  Results are
    /// ordered newest-first; capped at `limit` rows.
    async fn list_recent_for_owner(
        &self,
        owner_id: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<RecentActivityEntry>, EmberTroveError>;

    /// Every row, all owners, oldest first, with the action as its raw
    /// string. Admin backup only (#208): History must survive a restore.
    async fn list_all(&self) -> Result<Vec<ActivityRecord>, EmberTroveError>;

    /// The newest entry of one action on a node, if any. Edit coalescing
    /// looks at the last *Edited* entry, so other entries in between (a
    /// status change) don't split an editing session (#208).
    async fn latest_of(
        &self,
        node_id: NodeId,
        action: ActivityAction,
    ) -> Result<Option<ActivityEntry>, EmberTroveError>;
}

pub struct PgActivityRepo {
    pool: PgPool,
}

impl PgActivityRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct ActivityRow {
    id: Uuid,
    node_id: Uuid,
    subject_id: String,
    action: String,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl ActivityRow {
    /// `None` for an action this build doesn't know. The reads below
    /// already filter unknown actions out in SQL (#208); this is the
    /// backstop, so one stray row can never fail a whole list again.
    fn into_entry(self) -> Option<ActivityEntry> {
        let Some(action) = ActivityAction::from_db_str(&self.action) else {
            // The action name only — never the row's metadata (actor PII).
            tracing::warn!(action = %self.action, "skipping activity row with an unknown action");
            return None;
        };
        Some(ActivityEntry {
            id: ActivityId(self.id),
            node_id: NodeId(self.node_id),
            subject_id: self.subject_id,
            action,
            metadata: self.metadata,
            created_at: self.created_at,
        })
    }

    fn into_record(self) -> ActivityRecord {
        ActivityRecord {
            id: ActivityId(self.id),
            node_id: NodeId(self.node_id),
            subject_id: self.subject_id,
            action: self.action,
            metadata: self.metadata,
            created_at: self.created_at,
        }
    }
}

#[async_trait]
impl ActivityRepo for PgActivityRepo {
    async fn record(
        &self,
        node_id: NodeId,
        subject_id: &str,
        action: ActivityAction,
        metadata: serde_json::Value,
    ) -> Result<(), EmberTroveError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("record activity failed: {e}")))?;
        record_in(&mut conn, node_id, subject_id, action, metadata).await
    }

    async fn list(
        &self,
        node_id: NodeId,
        limit: i64,
    ) -> Result<Vec<ActivityEntry>, EmberTroveError> {
        // Unknown actions are filtered in SQL, before the LIMIT, so rows a
        // newer release wrote can't take the page's slots (#208).
        let rows = sqlx::query_as::<_, ActivityRow>(
            r#"
            SELECT id, node_id, subject_id, action, metadata, created_at
            FROM activity_log
            WHERE node_id = $1
              AND action = ANY($3)
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(node_id.inner())
        .bind(limit)
        .bind(ActivityAction::known_db_strs())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list activity failed: {e}")))?;

        Ok(rows
            .into_iter()
            .filter_map(ActivityRow::into_entry)
            .collect())
    }

    async fn list_recent_for_owner(
        &self,
        owner_id: &str,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<RecentActivityEntry>, EmberTroveError> {
        // Join activity_log to nodes so we can both filter by ownership
        // and surface the node title in one round-trip — the dashboard
        // recap renders 20-40 lines and otherwise we'd N+1 the title
        // lookup.
        #[derive(sqlx::FromRow)]
        struct Row {
            #[sqlx(flatten)]
            activity: ActivityRow,
            node_title: String,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT a.id, a.node_id, a.subject_id, a.action, a.metadata,
                   a.created_at, n.title AS node_title
            FROM activity_log a
            JOIN nodes n ON n.id = a.node_id
            WHERE n.owner_id = $1
              AND a.created_at >= $2
              AND a.action = ANY($4)
            ORDER BY a.created_at DESC
            LIMIT $3
            "#,
        )
        .bind(owner_id)
        .bind(since)
        .bind(limit)
        .bind(ActivityAction::known_db_strs())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_recent_for_owner failed: {e}")))?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(RecentActivityEntry {
                    entry: r.activity.into_entry()?,
                    node_title: r.node_title,
                })
            })
            .collect())
    }

    async fn list_all(&self) -> Result<Vec<ActivityRecord>, EmberTroveError> {
        let rows = sqlx::query_as::<_, ActivityRow>(
            r#"
            SELECT id, node_id, subject_id, action, metadata, created_at
            FROM activity_log
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all activity failed: {e}")))?;

        Ok(rows.into_iter().map(ActivityRow::into_record).collect())
    }

    async fn latest_of(
        &self,
        node_id: NodeId,
        action: ActivityAction,
    ) -> Result<Option<ActivityEntry>, EmberTroveError> {
        let row = sqlx::query_as::<_, ActivityRow>(
            r#"
            SELECT id, node_id, subject_id, action, metadata, created_at
            FROM activity_log
            WHERE node_id = $1 AND action = $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(node_id.inner())
        .bind(action.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("latest activity failed: {e}")))?;
        Ok(row.and_then(ActivityRow::into_entry))
    }
}
