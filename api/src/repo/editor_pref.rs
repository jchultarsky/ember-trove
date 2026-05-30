use async_trait::async_trait;
use common::{EmberTroveError, editor_pref::EditorPref};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait EditorPrefRepo: Send + Sync {
    /// All editor prefs for an owner (the UI fetches these once and applies
    /// heights by entity id).
    async fn list_for_owner(&self, owner_id: &str) -> Result<Vec<EditorPref>, EmberTroveError>;

    /// Upsert the height for one entity.
    async fn set(
        &self,
        owner_id: &str,
        entity_kind: &str,
        entity_id: Uuid,
        height: i32,
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
    height: i32,
}

#[async_trait]
impl EditorPrefRepo for PgEditorPrefRepo {
    async fn list_for_owner(&self, owner_id: &str) -> Result<Vec<EditorPref>, EmberTroveError> {
        let rows = sqlx::query_as::<_, PrefRow>(
            "SELECT entity_kind, entity_id, height FROM editor_prefs WHERE owner_id = $1",
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
            })
            .collect())
    }

    async fn set(
        &self,
        owner_id: &str,
        entity_kind: &str,
        entity_id: Uuid,
        height: i32,
    ) -> Result<(), EmberTroveError> {
        sqlx::query(
            r#"
            INSERT INTO editor_prefs (owner_id, entity_kind, entity_id, height)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (owner_id, entity_kind, entity_id)
            DO UPDATE SET height = EXCLUDED.height
            "#,
        )
        .bind(owner_id)
        .bind(entity_kind)
        .bind(entity_id)
        .bind(height)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("set editor pref failed: {e}")))?;
        Ok(())
    }
}
