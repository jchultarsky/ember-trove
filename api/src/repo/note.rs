use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, NoteId},
    note::{CreateNoteRequest, FeedNote, Note, UpdateNoteRequest},
};
use sqlx::PgPool;
use uuid::Uuid;

// ── Trait ──────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait NoteRepo: Send + Sync {
    /// Create a new note. `node_id: None` creates a standalone (inbox) note.
    async fn create(
        &self,
        node_id: Option<NodeId>,
        owner_id: &str,
        req: CreateNoteRequest,
    ) -> Result<Note, EmberTroveError>;

    /// Update the body and colour of an existing note. Only the note's owner may edit it.
    async fn update(
        &self,
        note_id: NoteId,
        owner_id: &str,
        req: UpdateNoteRequest,
    ) -> Result<Note, EmberTroveError>;

    /// All notes for a node, newest first.
    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Note>, EmberTroveError>;

    /// All notes by a given owner, newest first, with node titles (central feed).
    async fn feed_for_owner(&self, owner_id: &str) -> Result<Vec<FeedNote>, EmberTroveError>;

    /// All notes across all owners, newest first, with node titles.
    async fn feed_all(&self) -> Result<Vec<FeedNote>, EmberTroveError>;

    /// All notes across all owners — used for full backup.
    async fn list_all(&self) -> Result<Vec<Note>, EmberTroveError>;
}

// ── PgNoteRepo ─────────────────────────────────────────────────────────────────

pub struct PgNoteRepo {
    pool: PgPool,
}

impl PgNoteRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct NoteRow {
    id: Uuid,
    node_id: Option<Uuid>,
    owner_id: String,
    body: String,
    color: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl NoteRow {
    fn into_note(self) -> Note {
        Note {
            id: NoteId(self.id),
            node_id: self.node_id.map(NodeId),
            owner_id: self.owner_id,
            body: self.body,
            color: self.color,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Feed row = a note plus its parent node's title (`None` for standalone notes,
/// surfaced via the `LEFT JOIN nodes` in the feed queries).
#[derive(sqlx::FromRow)]
struct FeedRow {
    id: Uuid,
    node_id: Option<Uuid>,
    owner_id: String,
    body: String,
    color: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    node_title: Option<String>,
}

impl FeedRow {
    fn into_feed_note(self) -> FeedNote {
        FeedNote {
            note: Note {
                id: NoteId(self.id),
                node_id: self.node_id.map(NodeId),
                owner_id: self.owner_id,
                body: self.body,
                color: self.color,
                created_at: self.created_at,
                updated_at: self.updated_at,
            },
            node_title: self.node_title,
        }
    }
}

#[async_trait]
impl NoteRepo for PgNoteRepo {
    async fn create(
        &self,
        node_id: Option<NodeId>,
        owner_id: &str,
        req: CreateNoteRequest,
    ) -> Result<Note, EmberTroveError> {
        let row = sqlx::query_as::<_, NoteRow>(
            r#"
            INSERT INTO node_notes (node_id, owner_id, body, color)
            VALUES ($1, $2, $3, $4)
            RETURNING id, node_id, owner_id, body, color, created_at, updated_at
            "#,
        )
        .bind(node_id.map(|n| n.0))
        .bind(owner_id)
        .bind(&req.body)
        .bind(&req.color)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("create note failed: {e}")))?;

        Ok(row.into_note())
    }

    async fn update(
        &self,
        note_id: NoteId,
        owner_id: &str,
        req: UpdateNoteRequest,
    ) -> Result<Note, EmberTroveError> {
        let row = sqlx::query_as::<_, NoteRow>(
            r#"
            UPDATE node_notes
            SET body = $1, color = $2
            WHERE id = $3 AND owner_id = $4
            RETURNING id, node_id, owner_id, body, color, created_at, updated_at
            "#,
        )
        .bind(&req.body)
        .bind(&req.color)
        .bind(note_id.0)
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("update note failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound("note not found".to_string()))?;

        Ok(row.into_note())
    }

    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Note>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NoteRow>(
            r#"
            SELECT id, node_id, owner_id, body, color, created_at, updated_at
            FROM node_notes
            WHERE node_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(node_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list notes failed: {e}")))?;

        Ok(rows.into_iter().map(NoteRow::into_note).collect())
    }

    async fn list_all(&self) -> Result<Vec<Note>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NoteRow>(
            r#"
            SELECT id, node_id, owner_id, body, color, created_at, updated_at
            FROM node_notes
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all notes failed: {e}")))?;

        Ok(rows.into_iter().map(NoteRow::into_note).collect())
    }

    async fn feed_for_owner(&self, owner_id: &str) -> Result<Vec<FeedNote>, EmberTroveError> {
        let rows = sqlx::query_as::<_, FeedRow>(
            r#"
            SELECT
                n.id, n.node_id, n.owner_id, n.body, n.color, n.created_at, n.updated_at,
                nd.title AS node_title
            FROM node_notes n
            LEFT JOIN nodes nd ON nd.id = n.node_id
            WHERE n.owner_id = $1
            ORDER BY n.created_at DESC
            "#,
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("feed notes failed: {e}")))?;

        Ok(rows.into_iter().map(FeedRow::into_feed_note).collect())
    }

    async fn feed_all(&self) -> Result<Vec<FeedNote>, EmberTroveError> {
        let rows = sqlx::query_as::<_, FeedRow>(
            r#"
            SELECT
                n.id, n.node_id, n.owner_id, n.body, n.color, n.created_at, n.updated_at,
                nd.title AS node_title
            FROM node_notes n
            LEFT JOIN nodes nd ON nd.id = n.node_id
            ORDER BY n.created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("feed_all notes failed: {e}")))?;

        Ok(rows.into_iter().map(FeedRow::into_feed_note).collect())
    }
}
