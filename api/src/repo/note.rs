use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, NoteId},
    note::{CreateNoteRequest, FeedNote, Note, NoteSort, UpdateNoteRequest},
};
use sqlx::PgPool;
use uuid::Uuid;

/// Parsed filter for the notes feed (route maps `NoteFeedParams` → this).
#[derive(Debug, Clone, Default)]
pub struct NoteFeedFilter {
    /// Only notes attached to this node.
    pub node_id: Option<NodeId>,
    /// Only standalone (inbox) notes.
    pub uncategorized: bool,
    /// Inclusive lower bound on `created_at`.
    pub from: Option<DateTime<Utc>>,
    /// Exclusive upper bound on `created_at` (caller passes start-of-next-day).
    pub to: Option<DateTime<Utc>>,
    /// Case-insensitive substring filter on the body.
    pub q: Option<String>,
    pub sort: NoteSort,
    /// Max rows to return (server-clamped).
    pub limit: i64,
    /// Row offset for pagination.
    pub offset: i64,
}

/// Escape SQL `LIKE`/`ILIKE` metacharacters (`\`, `%`, `_`) so a user-supplied
/// substring is matched literally. Pair with `ESCAPE '\'` in the query.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

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

    /// Soft-delete a note (tombstone; restorable). Only the note's owner may
    /// delete it.
    async fn delete(&self, note_id: NoteId, owner_id: &str) -> Result<(), EmberTroveError>;

    /// Un-delete a tombstoned note. Owner-scoped like [`NoteRepo::delete`].
    async fn restore(&self, note_id: NoteId, owner_id: &str) -> Result<Note, EmberTroveError>;

    /// Hard-delete tombstones older than `cutoff`. Returns rows purged.
    async fn purge_deleted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EmberTroveError>;

    /// All notes for a node, newest first.
    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Note>, EmberTroveError>;

    /// Central feed: notes with node titles, filtered + sorted per `filter`.
    /// `owner_id: Some(sub)` scopes to one user; `None` returns all (admin).
    async fn feed(
        &self,
        owner_id: Option<&str>,
        filter: &NoteFeedFilter,
    ) -> Result<Vec<FeedNote>, EmberTroveError>;

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
            WHERE id = $3 AND owner_id = $4 AND deleted_at IS NULL
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

    async fn delete(&self, note_id: NoteId, owner_id: &str) -> Result<(), EmberTroveError> {
        // Soft delete: tombstone the row so the undo toast can restore it.
        let result = sqlx::query(
            "UPDATE node_notes SET deleted_at = now()
             WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL",
        )
        .bind(note_id.0)
        .bind(owner_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("delete note failed: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound("note not found".to_string()));
        }
        Ok(())
    }

    async fn restore(&self, note_id: NoteId, owner_id: &str) -> Result<Note, EmberTroveError> {
        let row = sqlx::query_as::<_, NoteRow>(
            r#"
            UPDATE node_notes SET deleted_at = NULL
            WHERE id = $1 AND owner_id = $2 AND deleted_at IS NOT NULL
            RETURNING id, node_id, owner_id, body, color, created_at, updated_at
            "#,
        )
        .bind(note_id.0)
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("restore note failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound("deleted note not found".to_string()))?;

        Ok(row.into_note())
    }

    async fn purge_deleted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EmberTroveError> {
        let result =
            sqlx::query("DELETE FROM node_notes WHERE deleted_at IS NOT NULL AND deleted_at < $1")
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(|e| EmberTroveError::Internal(format!("purge notes failed: {e}")))?;
        Ok(result.rows_affected())
    }

    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Note>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NoteRow>(
            r#"
            SELECT id, node_id, owner_id, body, color, created_at, updated_at
            FROM node_notes
            WHERE node_id = $1
              AND deleted_at IS NULL
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
            WHERE deleted_at IS NULL
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all notes failed: {e}")))?;

        Ok(rows.into_iter().map(NoteRow::into_note).collect())
    }

    async fn feed(
        &self,
        owner_id: Option<&str>,
        filter: &NoteFeedFilter,
    ) -> Result<Vec<FeedNote>, EmberTroveError> {
        // ORDER BY can't be a bind param; the column/direction come from a
        // closed enum (no injection). Filters use the guarded `$n IS NULL OR`
        // pattern so absent filters are no-ops.
        let order_by = match filter.sort {
            NoteSort::Newest => "n.created_at DESC",
            NoteSort::Oldest => "n.created_at ASC",
            NoteSort::Updated => "n.updated_at DESC",
        };
        let sql = format!(
            r#"
            SELECT
                n.id, n.node_id, n.owner_id, n.body, n.color, n.created_at, n.updated_at,
                nd.title AS node_title
            FROM node_notes n
            LEFT JOIN nodes nd ON nd.id = n.node_id
            WHERE n.deleted_at IS NULL
              AND ($1::text IS NULL OR n.owner_id = $1)
              AND ($2::uuid IS NULL OR n.node_id = $2)
              AND (NOT $3 OR n.node_id IS NULL)
              AND ($4::timestamptz IS NULL OR n.created_at >= $4)
              AND ($5::timestamptz IS NULL OR n.created_at < $5)
              AND ($6::text IS NULL OR n.body ILIKE ('%' || $6 || '%') ESCAPE '\')
            ORDER BY {order_by}
            LIMIT $7 OFFSET $8
            "#
        );

        // Escape LIKE metacharacters so a `q` of "%" / "_" matches literally
        // rather than acting as a wildcard (it bypasses the filter otherwise).
        let q_escaped = filter.q.as_deref().map(escape_like);

        let rows = sqlx::query_as::<_, FeedRow>(&sql)
            .bind(owner_id)
            .bind(filter.node_id.map(|n| n.0))
            .bind(filter.uncategorized)
            .bind(filter.from)
            .bind(filter.to)
            .bind(q_escaped)
            .bind(filter.limit)
            .bind(filter.offset)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("feed notes failed: {e}")))?;

        Ok(rows.into_iter().map(FeedRow::into_feed_note).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn escape_like_neutralizes_wildcards() {
        // `%` and `_` must be escaped so they match literally, not as wildcards.
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("%"), "\\%");
    }

    #[test]
    fn escape_like_escapes_backslash_first() {
        // The backslash itself must be doubled before it can escape the others,
        // otherwise the inserted escapes could be mis-parsed.
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("\\%"), "\\\\\\%");
    }

    #[test]
    fn escape_like_leaves_plain_text_untouched() {
        assert_eq!(escape_like("hello world"), "hello world");
        assert_eq!(escape_like(""), "");
    }
}
