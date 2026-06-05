use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    attachment::Attachment,
    id::{AttachmentId, NodeId},
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[async_trait]
pub trait AttachmentRepo: Send + Sync {
    async fn create(
        &self,
        node_id: NodeId,
        filename: &str,
        content_type: &str,
        size_bytes: i64,
        s3_key: &str,
    ) -> Result<Attachment, EmberTroveError>;

    async fn list(&self, node_id: NodeId) -> Result<Vec<Attachment>, EmberTroveError>;

    /// Fetch every attachment across all nodes in one query — used for full
    /// backup, replacing a per-node `list` N+1 loop.
    async fn list_all(&self) -> Result<Vec<Attachment>, EmberTroveError>;

    async fn get(&self, id: AttachmentId) -> Result<Attachment, EmberTroveError>;

    /// Delete the DB record and return the s3_key so the caller can remove the object.
    async fn delete(&self, id: AttachmentId) -> Result<String, EmberTroveError>;
}

#[derive(FromRow)]
struct AttachmentRow {
    id: Uuid,
    node_id: Uuid,
    filename: String,
    content_type: String,
    size_bytes: i64,
    s3_key: String,
    created_at: DateTime<Utc>,
}

impl From<AttachmentRow> for Attachment {
    fn from(r: AttachmentRow) -> Self {
        Self {
            id: AttachmentId(r.id),
            node_id: NodeId(r.node_id),
            filename: r.filename,
            content_type: r.content_type,
            size_bytes: r.size_bytes,
            s3_key: r.s3_key,
            created_at: r.created_at,
        }
    }
}

pub struct PgAttachmentRepo {
    pool: PgPool,
}

impl PgAttachmentRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AttachmentRepo for PgAttachmentRepo {
    async fn create(
        &self,
        node_id: NodeId,
        filename: &str,
        content_type: &str,
        size_bytes: i64,
        s3_key: &str,
    ) -> Result<Attachment, EmberTroveError> {
        let row = sqlx::query_as::<_, AttachmentRow>(
            r#"
            INSERT INTO attachments (node_id, filename, content_type, size_bytes, s3_key)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, node_id, filename, content_type, size_bytes, s3_key, created_at
            "#,
        )
        .bind(node_id.0)
        .bind(filename)
        .bind(content_type)
        .bind(size_bytes)
        .bind(s3_key)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("attachment create: {e}")))?;
        Ok(row.into())
    }

    async fn list(&self, node_id: NodeId) -> Result<Vec<Attachment>, EmberTroveError> {
        let rows = sqlx::query_as::<_, AttachmentRow>(
            r#"
            SELECT id, node_id, filename, content_type, size_bytes, s3_key, created_at
            FROM attachments
            WHERE node_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(node_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("attachment list: {e}")))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_all(&self) -> Result<Vec<Attachment>, EmberTroveError> {
        let rows = sqlx::query_as::<_, AttachmentRow>(
            r#"
            SELECT id, node_id, filename, content_type, size_bytes, s3_key, created_at
            FROM attachments
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("attachment list_all: {e}")))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get(&self, id: AttachmentId) -> Result<Attachment, EmberTroveError> {
        let row = sqlx::query_as::<_, AttachmentRow>(
            r#"
            SELECT id, node_id, filename, content_type, size_bytes, s3_key, created_at
            FROM attachments
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("attachment get: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("attachment {}", id.0)))?;
        Ok(row.into())
    }

    async fn delete(&self, id: AttachmentId) -> Result<String, EmberTroveError> {
        let s3_key: String =
            sqlx::query_scalar("DELETE FROM attachments WHERE id = $1 RETURNING s3_key")
                .bind(id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| EmberTroveError::Internal(format!("attachment delete: {e}")))?
                .ok_or_else(|| EmberTroveError::NotFound(format!("attachment {}", id.0)))?;
        Ok(s3_key)
    }
}
