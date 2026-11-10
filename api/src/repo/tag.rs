use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, TagId},
    tag::{CreateTagRequest, Tag, UpdateTagRequest},
};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait TagRepo: Send + Sync {
    async fn create(&self, owner_id: &str, req: CreateTagRequest) -> Result<Tag, EmberTroveError>;
    async fn get(&self, id: TagId) -> Result<Tag, EmberTroveError>;
    async fn list(&self, owner_id: &str) -> Result<Vec<Tag>, EmberTroveError>;
    /// All tags across all owners — used for full backup.
    async fn list_all(&self) -> Result<Vec<Tag>, EmberTroveError>;
    async fn update(&self, id: TagId, req: UpdateTagRequest) -> Result<Tag, EmberTroveError>;
    async fn delete(&self, id: TagId) -> Result<(), EmberTroveError>;
    async fn attach(&self, node_id: NodeId, tag_id: TagId) -> Result<(), EmberTroveError>;
    async fn detach(&self, node_id: NodeId, tag_id: TagId) -> Result<(), EmberTroveError>;
    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Tag>, EmberTroveError>;
}

pub struct PgTagRepo {
    pool: PgPool,
}

impl PgTagRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct TagRow {
    id: Uuid,
    owner_id: String,
    name: String,
    color: String,
    created_at: DateTime<Utc>,
}

impl TagRow {
    fn into_tag(self) -> Tag {
        Tag {
            id: TagId(self.id),
            owner_id: self.owner_id,
            name: self.name,
            color: self.color,
            created_at: self.created_at,
        }
    }
}

/// Every tag, all owners. A free function so the backup can run it on its one
/// snapshot transaction (`backup::read_snapshot`).
pub(crate) async fn list_all_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<Tag>, EmberTroveError> {
    let rows = sqlx::query_as::<_, TagRow>(
        r#"
        SELECT id, owner_id, name, color, created_at
        FROM tags
        ORDER BY name
        "#,
    )
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list_all tags failed: {e}")))?;

    Ok(rows.into_iter().map(TagRow::into_tag).collect())
}

#[async_trait]
impl TagRepo for PgTagRepo {
    async fn create(&self, owner_id: &str, req: CreateTagRequest) -> Result<Tag, EmberTroveError> {
        let row = sqlx::query_as::<_, TagRow>(
            r#"
            INSERT INTO tags (owner_id, name, color)
            VALUES ($1, $2, $3)
            RETURNING id, owner_id, name, color, created_at
            "#,
        )
        .bind(owner_id)
        .bind(&req.name)
        .bind(&req.color)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err)
                if db_err.constraint() == Some("tags_owner_id_name_key") =>
            {
                EmberTroveError::AlreadyExists(format!("tag '{}' already exists", req.name))
            }
            _ => EmberTroveError::Internal(format!("create tag failed: {e}")),
        })?;

        Ok(row.into_tag())
    }

    async fn get(&self, id: TagId) -> Result<Tag, EmberTroveError> {
        let row = sqlx::query_as::<_, TagRow>(
            r#"
            SELECT id, owner_id, name, color, created_at
            FROM tags
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get tag failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("tag {} not found", id.0)))?;

        Ok(row.into_tag())
    }

    async fn list(&self, owner_id: &str) -> Result<Vec<Tag>, EmberTroveError> {
        let rows = sqlx::query_as::<_, TagRow>(
            r#"
            SELECT id, owner_id, name, color, created_at
            FROM tags
            WHERE owner_id = $1
            ORDER BY name
            "#,
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list tags failed: {e}")))?;

        Ok(rows.into_iter().map(TagRow::into_tag).collect())
    }

    async fn list_all(&self) -> Result<Vec<Tag>, EmberTroveError> {
        list_all_on(&self.pool).await
    }

    async fn update(&self, id: TagId, req: UpdateTagRequest) -> Result<Tag, EmberTroveError> {
        let row = sqlx::query_as::<_, TagRow>(
            r#"
            UPDATE tags SET
                name  = COALESCE($2, name),
                color = COALESCE($3, color)
            WHERE id = $1
            RETURNING id, owner_id, name, color, created_at
            "#,
        )
        .bind(id.0)
        .bind(&req.name)
        .bind(&req.color)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err)
                if db_err.constraint() == Some("tags_owner_id_name_key") =>
            {
                EmberTroveError::AlreadyExists(format!(
                    "tag name already exists: {}",
                    req.name.as_deref().unwrap_or("?")
                ))
            }
            _ => EmberTroveError::Internal(format!("update tag failed: {e}")),
        })?
        .ok_or_else(|| EmberTroveError::NotFound(format!("tag {id} not found")))?;

        Ok(row.into_tag())
    }

    async fn delete(&self, id: TagId) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM tags WHERE id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete tag failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!("tag {id} not found")));
        }

        Ok(())
    }

    async fn attach(&self, node_id: NodeId, tag_id: TagId) -> Result<(), EmberTroveError> {
        sqlx::query(
            r#"
            INSERT INTO node_tags (node_id, tag_id)
            VALUES ($1, $2)
            ON CONFLICT (node_id, tag_id) DO NOTHING
            "#,
        )
        .bind(node_id.0)
        .bind(tag_id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.is_foreign_key_violation() => {
                EmberTroveError::NotFound("node or tag not found".to_string())
            }
            _ => EmberTroveError::Internal(format!("attach tag failed: {e}")),
        })?;

        Ok(())
    }

    async fn detach(&self, node_id: NodeId, tag_id: TagId) -> Result<(), EmberTroveError> {
        sqlx::query(
            r#"
            DELETE FROM node_tags
            WHERE node_id = $1 AND tag_id = $2
            "#,
        )
        .bind(node_id.0)
        .bind(tag_id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("detach tag failed: {e}")))?;

        Ok(())
    }

    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Tag>, EmberTroveError> {
        let rows = sqlx::query_as::<_, TagRow>(
            r#"
            SELECT t.id, t.owner_id, t.name, t.color, t.created_at
            FROM tags t
            JOIN node_tags nt ON nt.tag_id = t.id
            WHERE nt.node_id = $1
            ORDER BY t.name
            "#,
        )
        .bind(node_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list tags for node failed: {e}")))?;

        Ok(rows.into_iter().map(TagRow::into_tag).collect())
    }
}
