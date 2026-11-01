//! Captures (ONTOLOGY §9.1, #234): the Inbox's unprocessed input.
//!
//! SECURITY: owner-only, with no admin bypass (owner decision). Every
//! method takes the owner as `&str`, never `Option`, and every query
//! matches `owner_id`, so another account (an admin included) gets
//! `NotFound`, never a hint that the capture exists. Only `list_all`, for
//! the admin-run backup, reads across owners.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    capture::Capture,
    id::{CaptureId, NodeId},
    node::{CreateNodeRequest, Node},
    note::{CreateNoteRequest, Note},
    permission::{GrantPermissionRequest, PermissionRole},
    task::{CreateTaskRequest, Task},
};
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
    /// Turn the owner's live capture into exactly one destination (§9.1)
    /// in one transaction: the capture is hard-deleted first (no row:
    /// `NotFound`, nothing made), then the destination is inserted. Any
    /// failure rolls both back, so the text is never lost nor doubled.
    /// The caller has already checked access and kinds.
    async fn convert(
        &self,
        id: CaptureId,
        owner: &str,
        into: Destination,
    ) -> Result<Made, EmberTroveError>;
}

/// What a capture becomes, ready to insert: the route has validated it,
/// checked the caller's rights, and set a Project's initial status.
#[derive(Debug, Clone)]
pub enum Destination {
    Node {
        req: CreateNodeRequest,
        home: Option<NodeId>,
    },
    Note {
        node_id: NodeId,
        req: CreateNoteRequest,
    },
    Task {
        node_id: NodeId,
        req: CreateTaskRequest,
    },
}

/// The destination as inserted, for the route's follow-up (activity,
/// webhooks, wiki links) and its answer.
#[derive(Debug, Clone)]
pub enum Made {
    Node(Box<Node>),
    Note(Note),
    /// The Task and the Project it was made in.
    Task {
        project: NodeId,
        task: Task,
    },
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

    async fn convert(
        &self,
        id: CaptureId,
        owner: &str,
        into: Destination,
    ) -> Result<Made, EmberTroveError> {
        let mut tx = self.pool.begin().await.map_err(db("convert capture"))?;
        // First, so a missing (or someone else's, or binned) capture makes
        // nothing; the row lock also stops a second convert of it. A hard
        // delete: no provenance is kept (§9.1).
        let gone = sqlx::query(
            "DELETE FROM captures WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
        )
        .bind(id.0)
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(db("convert capture"))?;
        if gone.rows_affected() == 0 {
            return Err(not_found());
        }
        let made = match into {
            Destination::Node { req, home } => {
                let node = super::node::insert_node(&mut *tx, owner, &req).await?;
                let grant = GrantPermissionRequest {
                    subject_id: owner.to_string(),
                    role: PermissionRole::Owner,
                };
                super::permission::insert_grant(&mut *tx, node.id, owner, &grant).await?;
                if let Some(parent) = home {
                    // A new node has no children, so no loop can pass
                    // through it, and no other home: the `contains` lock
                    // isn't needed.
                    super::edge::insert_home(&mut *tx, parent, node.id).await?;
                }
                Made::Node(Box::new(node))
            }
            Destination::Note { node_id, req } => {
                Made::Note(super::note::insert_note(&mut *tx, node_id, owner, &req).await?)
            }
            Destination::Task { node_id, req } => Made::Task {
                project: node_id,
                task: super::task::insert_task(&mut *tx, Some(node_id), owner, &req, None).await?,
            },
        };
        tx.commit().await.map_err(db("convert capture"))?;
        Ok(made)
    }
}
