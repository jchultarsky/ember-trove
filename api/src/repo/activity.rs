use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    activity::{ActivityAction, ActivityEntry, RecentActivityEntry},
    id::{ActivityId, NodeId},
};
use sqlx::PgPool;
use uuid::Uuid;

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
    fn into_entry(self) -> Result<ActivityEntry, EmberTroveError> {
        let action = ActivityAction::from_db_str(&self.action).ok_or_else(|| {
            EmberTroveError::Internal(format!("unknown activity action: {}", self.action))
        })?;
        Ok(ActivityEntry {
            id: ActivityId(self.id),
            node_id: NodeId(self.node_id),
            subject_id: self.subject_id,
            action,
            metadata: self.metadata,
            created_at: self.created_at,
        })
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
        sqlx::query(
            r#"
            INSERT INTO activity_log (node_id, subject_id, action, metadata)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(node_id.inner())
        .bind(subject_id)
        .bind(action.as_str())
        .bind(metadata)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("record activity failed: {e}")))?;
        Ok(())
    }

    async fn list(
        &self,
        node_id: NodeId,
        limit: i64,
    ) -> Result<Vec<ActivityEntry>, EmberTroveError> {
        let rows = sqlx::query_as::<_, ActivityRow>(
            r#"
            SELECT id, node_id, subject_id, action, metadata, created_at
            FROM activity_log
            WHERE node_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(node_id.inner())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list activity failed: {e}")))?;

        rows.into_iter().map(|r| r.into_entry()).collect()
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
            id: Uuid,
            node_id: Uuid,
            subject_id: String,
            action: String,
            metadata: serde_json::Value,
            created_at: DateTime<Utc>,
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
            ORDER BY a.created_at DESC
            LIMIT $3
            "#,
        )
        .bind(owner_id)
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_recent_for_owner failed: {e}")))?;

        rows.into_iter()
            .map(|r| {
                let action = ActivityAction::from_db_str(&r.action).ok_or_else(|| {
                    EmberTroveError::Internal(format!("unknown activity action: {}", r.action))
                })?;
                Ok(RecentActivityEntry {
                    entry: ActivityEntry {
                        id: ActivityId(r.id),
                        node_id: NodeId(r.node_id),
                        subject_id: r.subject_id,
                        action,
                        metadata: r.metadata,
                        created_at: r.created_at,
                    },
                    node_title: r.node_title,
                })
            })
            .collect()
    }
}
