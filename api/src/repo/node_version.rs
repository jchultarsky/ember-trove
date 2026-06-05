use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, NodeVersionId},
    node_version::NodeVersion,
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[async_trait]
pub trait NodeVersionRepo: Send + Sync + 'static {
    /// All versions across all nodes, for backup purposes.
    async fn list_all(&self) -> Result<Vec<NodeVersion>, EmberTroveError>;

    /// Record a snapshot of `body` for `node_id`, attributed to `created_by`.
    async fn record(
        &self,
        node_id: NodeId,
        body: &str,
        created_by: &str,
    ) -> Result<(), EmberTroveError>;

    /// List up to `limit` most-recent versions for a node, newest first.
    async fn list(&self, node_id: NodeId, limit: i64) -> Result<Vec<NodeVersion>, EmberTroveError>;

    /// Fetch a single version by ID.
    async fn get(&self, id: NodeVersionId) -> Result<NodeVersion, EmberTroveError>;
}

// ── Internal row type ─────────────────────────────────────────────────────────

#[derive(FromRow)]
struct NodeVersionRow {
    id: Uuid,
    node_id: Uuid,
    body: String,
    created_by: String,
    created_at: DateTime<Utc>,
}

impl NodeVersionRow {
    fn into_version(self) -> NodeVersion {
        NodeVersion {
            id: NodeVersionId(self.id),
            node_id: NodeId(self.node_id),
            body: self.body,
            created_by: self.created_by,
            created_at: self.created_at,
        }
    }
}

// ── Postgres implementation ───────────────────────────────────────────────────

pub struct PgNodeVersionRepo {
    pool: PgPool,
}

impl PgNodeVersionRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl NodeVersionRepo for PgNodeVersionRepo {
    async fn list_all(&self) -> Result<Vec<NodeVersion>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NodeVersionRow>(
            "SELECT id, node_id, body, created_by, created_at
             FROM node_versions ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all node_versions failed: {e}")))?;

        Ok(rows.into_iter().map(NodeVersionRow::into_version).collect())
    }

    async fn record(
        &self,
        node_id: NodeId,
        body: &str,
        created_by: &str,
    ) -> Result<(), EmberTroveError> {
        sqlx::query("INSERT INTO node_versions (node_id, body, created_by) VALUES ($1, $2, $3)")
            .bind(node_id.0)
            .bind(body)
            .bind(created_by)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("record node version failed: {e}")))?;
        Ok(())
    }

    async fn list(&self, node_id: NodeId, limit: i64) -> Result<Vec<NodeVersion>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NodeVersionRow>(
            r#"SELECT id, node_id, body, created_by, created_at
               FROM node_versions
               WHERE node_id = $1
               ORDER BY created_at DESC
               LIMIT $2"#,
        )
        .bind(node_id.0)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list node versions failed: {e}")))?;

        Ok(rows.into_iter().map(NodeVersionRow::into_version).collect())
    }

    async fn get(&self, id: NodeVersionId) -> Result<NodeVersion, EmberTroveError> {
        sqlx::query_as::<_, NodeVersionRow>(
            "SELECT id, node_id, body, created_by, created_at FROM node_versions WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get node version failed: {e}")))?
        .map(NodeVersionRow::into_version)
        .ok_or_else(|| EmberTroveError::NotFound("node version not found".to_string()))
    }
}
