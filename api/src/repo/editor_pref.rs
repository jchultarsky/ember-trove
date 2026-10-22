use async_trait::async_trait;
use common::{EmberTroveError, editor_pref::EditorPref};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait EditorPrefRepo: Send + Sync {
    /// All editor prefs for an owner (the UI fetches these once and applies
    /// heights / collapse state by entity id).
    async fn list_for_owner(&self, owner_id: &str) -> Result<Vec<EditorPref>, EmberTroveError>;

    /// Upsert one entity's prefs. A `None` field keeps its stored value (or
    /// the column default for a new row), so saving a height never resets
    /// `collapsed` and vice versa.
    async fn set(
        &self,
        owner_id: &str,
        entity_kind: &str,
        entity_id: Uuid,
        height: Option<i32>,
        collapsed: Option<bool>,
    ) -> Result<(), EmberTroveError>;

    /// Set `collapsed` for many entities at once (Collapse all / Expand all).
    /// Collapsing upserts a row per entity; expanding only updates existing
    /// rows, since a missing row already means "expanded".
    async fn set_collapsed_many(
        &self,
        owner_id: &str,
        entity_kind: &str,
        entity_ids: &[Uuid],
        collapsed: bool,
    ) -> Result<(), EmberTroveError>;
}

pub struct PgEditorPrefRepo {
    pool: PgPool,
}

impl PgEditorPrefRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct PrefRow {
    entity_kind: String,
    entity_id: Uuid,
    height: Option<i32>,
    collapsed: bool,
}

#[async_trait]
impl EditorPrefRepo for PgEditorPrefRepo {
    async fn list_for_owner(&self, owner_id: &str) -> Result<Vec<EditorPref>, EmberTroveError> {
        let rows = sqlx::query_as::<_, PrefRow>(
            "SELECT entity_kind, entity_id, height, collapsed
             FROM editor_prefs WHERE owner_id = $1",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list editor prefs failed: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| EditorPref {
                entity_kind: r.entity_kind,
                entity_id: r.entity_id,
                height: r.height,
                collapsed: r.collapsed,
            })
            .collect())
    }

    async fn set(
        &self,
        owner_id: &str,
        entity_kind: &str,
        entity_id: Uuid,
        height: Option<i32>,
        collapsed: Option<bool>,
    ) -> Result<(), EmberTroveError> {
        sqlx::query(
            r#"
            INSERT INTO editor_prefs (owner_id, entity_kind, entity_id, height, collapsed)
            VALUES ($1, $2, $3, $4, COALESCE($5, FALSE))
            ON CONFLICT (owner_id, entity_kind, entity_id)
            DO UPDATE SET height    = COALESCE($4, editor_prefs.height),
                          collapsed = COALESCE($5, editor_prefs.collapsed)
            "#,
        )
        .bind(owner_id)
        .bind(entity_kind)
        .bind(entity_id)
        .bind(height)
        .bind(collapsed)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("set editor pref failed: {e}")))?;
        Ok(())
    }

    async fn set_collapsed_many(
        &self,
        owner_id: &str,
        entity_kind: &str,
        entity_ids: &[Uuid],
        collapsed: bool,
    ) -> Result<(), EmberTroveError> {
        if entity_ids.is_empty() {
            return Ok(());
        }
        let query = if collapsed {
            // DISTINCT: a repeated id would otherwise make ON CONFLICT touch the
            // same row twice in one statement, which Postgres rejects.
            r#"
            INSERT INTO editor_prefs (owner_id, entity_kind, entity_id, collapsed)
            SELECT DISTINCT $1, $2, id, TRUE FROM UNNEST($3::uuid[]) AS t(id)
            ON CONFLICT (owner_id, entity_kind, entity_id)
            DO UPDATE SET collapsed = TRUE
            "#
        } else {
            r#"
            UPDATE editor_prefs SET collapsed = FALSE
            WHERE owner_id = $1 AND entity_kind = $2 AND entity_id = ANY($3::uuid[])
            "#
        };
        sqlx::query(query)
            .bind(owner_id)
            .bind(entity_kind)
            .bind(entity_ids)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                EmberTroveError::Internal(format!("set collapsed editor prefs failed: {e}"))
            })?;
        Ok(())
    }
}
