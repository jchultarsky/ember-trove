//! The S3 deletion outbox (#239): objects whose rows are gone, waiting to
//! be deleted from storage. A row is written in the same transaction that
//! deletes the row naming the object, so a crash or an S3 failure leaves
//! it for the next try instead of an orphaned object.

use async_trait::async_trait;
use common::EmberTroveError;
use sqlx::PgPool;

#[async_trait]
pub trait ObjectDeletionRepo: Send + Sync {
    /// Queue a key whose row never landed (an upload whose insert failed).
    async fn enqueue(&self, s3_key: &str) -> Result<(), EmberTroveError>;
    /// Up to `limit` queued keys, least recently tried first. Keys an
    /// attachment row names again (a restore put them back) leave the queue
    /// first and are never returned: storage must keep what a row names.
    async fn due(&self, limit: i64) -> Result<Vec<String>, EmberTroveError>;
    /// The object is gone: drop its row.
    async fn done(&self, s3_key: &str) -> Result<(), EmberTroveError>;
    /// The delete failed: count the try and keep the row.
    async fn failed(&self, s3_key: &str) -> Result<(), EmberTroveError>;
}

pub struct PgObjectDeletionRepo {
    pool: PgPool,
}

impl PgObjectDeletionRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn outbox_err(e: sqlx::Error) -> EmberTroveError {
    EmberTroveError::Internal(format!("object deletion queue failed: {e}"))
}

#[async_trait]
impl ObjectDeletionRepo for PgObjectDeletionRepo {
    async fn enqueue(&self, s3_key: &str) -> Result<(), EmberTroveError> {
        sqlx::query("INSERT INTO object_deletions (s3_key) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(s3_key)
            .execute(&self.pool)
            .await
            .map_err(outbox_err)?;
        Ok(())
    }

    async fn due(&self, limit: i64) -> Result<Vec<String>, EmberTroveError> {
        sqlx::query("DELETE FROM object_deletions d USING attachments a WHERE a.s3_key = d.s3_key")
            .execute(&self.pool)
            .await
            .map_err(outbox_err)?;
        sqlx::query_scalar(
            "SELECT s3_key FROM object_deletions
             ORDER BY last_attempt_at NULLS FIRST, enqueued_at
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(outbox_err)
    }

    async fn done(&self, s3_key: &str) -> Result<(), EmberTroveError> {
        sqlx::query("DELETE FROM object_deletions WHERE s3_key = $1")
            .bind(s3_key)
            .execute(&self.pool)
            .await
            .map_err(outbox_err)?;
        Ok(())
    }

    async fn failed(&self, s3_key: &str) -> Result<(), EmberTroveError> {
        sqlx::query(
            "UPDATE object_deletions SET attempts = attempts + 1, last_attempt_at = now()
             WHERE s3_key = $1",
        )
        .bind(s3_key)
        .execute(&self.pool)
        .await
        .map_err(outbox_err)?;
        Ok(())
    }
}
