use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    favorite::{CreateFavoriteRequest, Favorite},
    id::{FavoriteId, NodeId},
};
use sqlx::PgPool;
use uuid::Uuid;

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait FavoriteRepo: Send + Sync {
    /// All favorites across all users, for backup purposes.
    async fn list_all(&self) -> Result<Vec<Favorite>, EmberTroveError>;

    /// All favorites for `owner_id`, ordered by position ascending.
    async fn list(&self, owner_id: &str) -> Result<Vec<Favorite>, EmberTroveError>;

    /// Create a new favorite at the end of the list.
    async fn create(
        &self,
        owner_id: &str,
        req: CreateFavoriteRequest,
    ) -> Result<Favorite, EmberTroveError>;

    /// Delete a favorite. Returns `NotFound` if `id` doesn't belong to `owner_id`.
    async fn delete(&self, id: FavoriteId, owner_id: &str) -> Result<(), EmberTroveError>;

    /// Reorder favorites by writing `position = index` for each ID in order.
    /// IDs not belonging to `owner_id` are silently ignored.
    async fn reorder(
        &self,
        owner_id: &str,
        ids: &[FavoriteId],
    ) -> Result<Vec<Favorite>, EmberTroveError>;
}

// ── PgFavoriteRepo ────────────────────────────────────────────────────────────

pub struct PgFavoriteRepo {
    pool: PgPool,
}

impl PgFavoriteRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct FavoriteRow {
    id: Uuid,
    owner_id: String,
    node_id: Option<Uuid>,
    url: Option<String>,
    label: String,
    position: i32,
    created_at: DateTime<Utc>,
}

impl From<FavoriteRow> for Favorite {
    fn from(r: FavoriteRow) -> Self {
        Favorite {
            id: FavoriteId(r.id),
            owner_id: r.owner_id,
            node_id: r.node_id.map(NodeId),
            url: r.url,
            label: r.label,
            position: r.position,
            created_at: r.created_at,
        }
    }
}

#[async_trait]
impl FavoriteRepo for PgFavoriteRepo {
    async fn list_all(&self) -> Result<Vec<Favorite>, EmberTroveError> {
        let rows = sqlx::query_as::<_, FavoriteRow>(
            r#"
            SELECT f.id, f.owner_id, f.node_id, f.url, f.label,
                   f.position, f.created_at
            FROM user_favorites f
            ORDER BY f.owner_id, f.position ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all favorites failed: {e}")))?;

        Ok(rows.into_iter().map(Favorite::from).collect())
    }

    async fn list(&self, owner_id: &str) -> Result<Vec<Favorite>, EmberTroveError> {
        // COALESCE(n.title, f.label): node favorites show the node's live title;
        // URL favorites fall back to the stored label.
        let rows = sqlx::query_as::<_, FavoriteRow>(
            r#"
            SELECT
                f.id, f.owner_id, f.node_id, f.url,
                COALESCE(n.title, f.label) AS label,
                f.position, f.created_at
            FROM user_favorites f
            LEFT JOIN nodes n ON n.id = f.node_id
            WHERE f.owner_id = $1
            ORDER BY f.position ASC, f.created_at ASC
            "#,
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list favorites failed: {e}")))?;

        Ok(rows.into_iter().map(Favorite::from).collect())
    }

    async fn create(
        &self,
        owner_id: &str,
        req: CreateFavoriteRequest,
    ) -> Result<Favorite, EmberTroveError> {
        // Validate: exactly one of node_id or url must be set.
        match (&req.node_id, &req.url) {
            (Some(_), None) | (None, Some(_)) => {}
            _ => {
                return Err(EmberTroveError::Validation(
                    "exactly one of node_id or url must be provided".to_string(),
                ));
            }
        }

        if req.label.trim().is_empty() {
            return Err(EmberTroveError::Validation(
                "label must not be empty".to_string(),
            ));
        }

        let row = sqlx::query_as::<_, FavoriteRow>(
            r#"
            INSERT INTO user_favorites (owner_id, node_id, url, label, position)
            VALUES (
                $1, $2, $3, $4,
                (SELECT COALESCE(MAX(position), -1) + 1
                 FROM user_favorites WHERE owner_id = $1)
            )
            RETURNING
                id, owner_id, node_id, url,
                label,
                position, created_at
            "#,
        )
        .bind(owner_id)
        .bind(req.node_id)
        .bind(&req.url)
        .bind(req.label.trim())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") {
                EmberTroveError::AlreadyExists("favorite already exists".to_string())
            } else {
                EmberTroveError::Internal(format!("create favorite failed: {e}"))
            }
        })?;

        Ok(Favorite::from(row))
    }

    async fn delete(&self, id: FavoriteId, owner_id: &str) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM user_favorites WHERE id = $1 AND owner_id = $2")
            .bind(id.0)
            .bind(owner_id)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete favorite failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!(
                "favorite {id} not found"
            )));
        }
        Ok(())
    }

    async fn reorder(
        &self,
        owner_id: &str,
        ids: &[FavoriteId],
    ) -> Result<Vec<Favorite>, EmberTroveError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("begin tx failed: {e}")))?;

        for (pos, fav_id) in ids.iter().enumerate() {
            sqlx::query("UPDATE user_favorites SET position = $1 WHERE id = $2 AND owner_id = $3")
                .bind(pos as i32)
                .bind(fav_id.0)
                .bind(owner_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| EmberTroveError::Internal(format!("reorder favorite failed: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("commit reorder failed: {e}")))?;

        self.list(owner_id).await
    }
}
