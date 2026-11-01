//! Captures (ONTOLOGY §9.1, #234): the Inbox's unprocessed input.
//!
//! SECURITY: owner-only, with no admin bypass (owner decision). Every
//! method takes the owner as `&str`, never `Option`, and every query
//! matches `owner_id`, so another account (an admin included) gets
//! `NotFound`, never a hint that the capture exists. Only `list_all`, for
//! the admin-run backup, reads across owners.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{EmberTroveError, capture::Capture, id::CaptureId};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait CaptureRepo: Send + Sync {
    /// The owner's captures, newest first.
    async fn list(&self, owner: &str) -> Result<Vec<Capture>, EmberTroveError>;
    async fn create(&self, owner: &str, body: &str) -> Result<Capture, EmberTroveError>;
    async fn update(
        &self,
        id: CaptureId,
        owner: &str,
        body: &str,
    ) -> Result<Capture, EmberTroveError>;
    /// Soft delete (tombstone), so the Undo toast can bring it back.
    async fn delete(&self, id: CaptureId, owner: &str) -> Result<(), EmberTroveError>;
    async fn restore(&self, id: CaptureId, owner: &str) -> Result<Capture, EmberTroveError>;
    /// Hard-delete tombstones older than `cutoff`. Returns rows purged.
    async fn purge_deleted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EmberTroveError>;
    /// Every live capture of every owner: the admin-run backup only.
    async fn list_all(&self) -> Result<Vec<Capture>, EmberTroveError>;
}

pub struct PgCaptureRepo {
    pool: PgPool,
}

impl PgCaptureRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    owner_id: String,
    body: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for Capture {
    fn from(r: Row) -> Self {
        Capture {
            id: CaptureId(r.id),
            owner_id: r.owner_id,
            body: r.body,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const COLUMNS: &str = "id, owner_id, body, created_at, updated_at";

fn db(what: &'static str) -> impl Fn(sqlx::Error) -> EmberTroveError {
    move |e| EmberTroveError::Internal(format!("{what} failed: {e}"))
}

fn not_found() -> EmberTroveError {
    EmberTroveError::NotFound("capture not found".to_string())
}

#[async_trait]
impl CaptureRepo for PgCaptureRepo {
    async fn list(&self, owner: &str) -> Result<Vec<Capture>, EmberTroveError> {
        let rows: Vec<Row> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM captures \
             WHERE owner_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC, id"
        ))
        .bind(owner)
        .fetch_all(&self.pool)
        .await
        .map_err(db("list captures"))?;
        Ok(rows.into_iter().map(Capture::from).collect())
    }

    async fn create(&self, owner: &str, body: &str) -> Result<Capture, EmberTroveError> {
        let row: Row = sqlx::query_as(&format!(
            "INSERT INTO captures (owner_id, body) VALUES ($1, $2) RETURNING {COLUMNS}"
        ))
        .bind(owner)
        .bind(body)
        .fetch_one(&self.pool)
        .await
        .map_err(db("create capture"))?;
        Ok(row.into())
    }

    async fn update(
        &self,
        id: CaptureId,
        owner: &str,
        body: &str,
    ) -> Result<Capture, EmberTroveError> {
        let row: Option<Row> = sqlx::query_as(&format!(
            "UPDATE captures SET body = $3 \
             WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL \
             RETURNING {COLUMNS}"
        ))
        .bind(id.0)
        .bind(owner)
        .bind(body)
        .fetch_optional(&self.pool)
        .await
        .map_err(db("update capture"))?;
        row.map(Capture::from).ok_or_else(not_found)
    }

    async fn delete(&self, id: CaptureId, owner: &str) -> Result<(), EmberTroveError> {
        let done = sqlx::query(
            "UPDATE captures SET deleted_at = now() \
             WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
        )
        .bind(id.0)
        .bind(owner)
        .execute(&self.pool)
        .await
        .map_err(db("delete capture"))?;
        if done.rows_affected() == 0 {
            return Err(not_found());
        }
        Ok(())
    }

    async fn restore(&self, id: CaptureId, owner: &str) -> Result<Capture, EmberTroveError> {
        let row: Option<Row> = sqlx::query_as(&format!(
            "UPDATE captures SET deleted_at = NULL \
             WHERE id = $1 AND owner_id = $2 AND deleted_at IS NOT NULL \
             RETURNING {COLUMNS}"
        ))
        .bind(id.0)
        .bind(owner)
        .fetch_optional(&self.pool)
        .await
        .map_err(db("restore capture"))?;
        row.map(Capture::from).ok_or_else(not_found)
    }

    async fn purge_deleted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EmberTroveError> {
        let done =
            sqlx::query("DELETE FROM captures WHERE deleted_at IS NOT NULL AND deleted_at < $1")
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(db("purge captures"))?;
        Ok(done.rows_affected())
    }

    async fn list_all(&self) -> Result<Vec<Capture>, EmberTroveError> {
        let rows: Vec<Row> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM captures WHERE deleted_at IS NULL ORDER BY created_at, id"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(db("list all captures"))?;
        Ok(rows.into_iter().map(Capture::from).collect())
    }
}
