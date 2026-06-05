use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::SearchPresetId,
    search::{CreateSearchPresetRequest, SearchPreset},
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[async_trait]
pub trait SearchPresetRepo: Send + Sync + 'static {
    /// Return all presets owned by the given user, newest first.
    async fn list(&self, owner_id: &str) -> Result<Vec<SearchPreset>, EmberTroveError>;

    /// Create a new preset.
    async fn create(
        &self,
        owner_id: &str,
        req: CreateSearchPresetRequest,
    ) -> Result<SearchPreset, EmberTroveError>;

    /// Delete a preset — only owner may delete.
    async fn delete(&self, id: SearchPresetId, owner_id: &str) -> Result<(), EmberTroveError>;
}

// ── Internal row type ─────────────────────────────────────────────────────────

#[derive(FromRow)]
struct PresetRow {
    id: Uuid,
    owner_id: String,
    name: String,
    query: String,
    fuzzy: bool,
    published_only: bool,
    tag_ids: String,
    tag_op: String,
    created_at: DateTime<Utc>,
}

impl PresetRow {
    fn into_preset(self) -> SearchPreset {
        SearchPreset {
            id: SearchPresetId(self.id),
            owner_id: self.owner_id,
            name: self.name,
            query: self.query,
            fuzzy: self.fuzzy,
            published_only: self.published_only,
            tag_ids: self.tag_ids,
            tag_op: self.tag_op,
            created_at: self.created_at,
        }
    }
}

// ── Postgres implementation ───────────────────────────────────────────────────

pub struct PgSearchPresetRepo {
    pool: PgPool,
}

impl PgSearchPresetRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SearchPresetRepo for PgSearchPresetRepo {
    async fn list(&self, owner_id: &str) -> Result<Vec<SearchPreset>, EmberTroveError> {
        let rows = sqlx::query_as::<_, PresetRow>(
            "SELECT id, owner_id, name, query, fuzzy, published_only, tag_ids, tag_op, created_at \
             FROM search_presets \
             WHERE owner_id = $1 \
             ORDER BY created_at DESC",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list search presets failed: {e}")))?;

        Ok(rows.into_iter().map(PresetRow::into_preset).collect())
    }

    async fn create(
        &self,
        owner_id: &str,
        req: CreateSearchPresetRequest,
    ) -> Result<SearchPreset, EmberTroveError> {
        let row = sqlx::query_as::<_, PresetRow>(
            "INSERT INTO search_presets \
             (owner_id, name, query, fuzzy, published_only, tag_ids, tag_op) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, owner_id, name, query, fuzzy, published_only, tag_ids, tag_op, created_at",
        )
        .bind(owner_id)
        .bind(&req.name)
        .bind(&req.query)
        .bind(req.fuzzy)
        .bind(req.published_only)
        .bind(&req.tag_ids)
        .bind(&req.tag_op)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("create search preset failed: {e}")))?;

        Ok(row.into_preset())
    }

    async fn delete(&self, id: SearchPresetId, owner_id: &str) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM search_presets WHERE id = $1 AND owner_id = $2")
            .bind(id.0)
            .bind(owner_id)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete search preset failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(
                "search preset not found".to_string(),
            ));
        }
        Ok(())
    }
}
