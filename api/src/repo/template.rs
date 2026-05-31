use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::TemplateId,
    node::NodeType,
    template::{CreateTemplateRequest, NodeTemplate, UpdateTemplateRequest},
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[async_trait]
pub trait TemplateRepo: Send + Sync + 'static {
    /// Return templates ordered by name. SECURITY: scoped to `subject`
    /// (`Some(sub)`); pass `None` for admins to list across all owners.
    async fn list(&self, subject: Option<&str>) -> Result<Vec<NodeTemplate>, EmberTroveError>;

    /// Fetch a single template by ID.
    async fn get(&self, id: TemplateId) -> Result<NodeTemplate, EmberTroveError>;

    /// Create a new template.
    async fn create(
        &self,
        created_by: &str,
        req: CreateTemplateRequest,
    ) -> Result<NodeTemplate, EmberTroveError>;

    /// Update an existing template.
    async fn update(
        &self,
        id: TemplateId,
        req: UpdateTemplateRequest,
    ) -> Result<NodeTemplate, EmberTroveError>;

    /// Delete a template.
    async fn delete(&self, id: TemplateId) -> Result<(), EmberTroveError>;

    /// Toggle the default flag for `id` (owned by `created_by`).
    ///
    /// Runs in a transaction:
    /// 1. Clears `is_default` on all other templates with the same
    ///    `(created_by, node_type)`.
    /// 2. Flips `is_default` on the target template.
    ///
    /// Returns the updated template.  Returns `NotFound` if the template does
    /// not exist or is not owned by `created_by`.
    async fn set_default(
        &self,
        id: TemplateId,
        created_by: &str,
    ) -> Result<NodeTemplate, EmberTroveError>;
}

// ── Internal row type ─────────────────────────────────────────────────────────

#[derive(FromRow)]
struct TemplateRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    node_type: String,
    body: String,
    is_default: bool,
    created_by: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TemplateRow {
    fn into_template(self) -> Result<NodeTemplate, EmberTroveError> {
        let node_type = parse_node_type(&self.node_type)?;
        Ok(NodeTemplate {
            id: TemplateId(self.id),
            name: self.name,
            description: self.description,
            node_type,
            body: self.body,
            is_default: self.is_default,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn parse_node_type(s: &str) -> Result<NodeType, EmberTroveError> {
    match s {
        "article"   => Ok(NodeType::Article),
        "project"   => Ok(NodeType::Project),
        "area"      => Ok(NodeType::Area),
        "resource"  => Ok(NodeType::Resource),
        "reference" => Ok(NodeType::Reference),
        other => Err(EmberTroveError::Internal(format!(
            "unknown node_type in template: {other}"
        ))),
    }
}

fn node_type_str(nt: &NodeType) -> &'static str {
    match nt {
        NodeType::Article   => "article",
        NodeType::Project   => "project",
        NodeType::Area      => "area",
        NodeType::Resource  => "resource",
        NodeType::Reference => "reference",
    }
}

// ── Postgres implementation ───────────────────────────────────────────────────

pub struct PgTemplateRepo {
    pool: PgPool,
}

impl PgTemplateRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Columns returned by every SELECT / RETURNING in this repo.
const COLS: &str =
    "id, name, description, node_type, body, is_default, created_by, created_at, updated_at";

#[async_trait]
impl TemplateRepo for PgTemplateRepo {
    async fn list(&self, subject: Option<&str>) -> Result<Vec<NodeTemplate>, EmberTroveError> {
        let rows = sqlx::query_as::<_, TemplateRow>(&format!(
            "SELECT {COLS} FROM node_templates \
             WHERE ($1::text IS NULL OR created_by = $1::text) \
             ORDER BY name",
        ))
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list templates failed: {e}")))?;

        rows.into_iter().map(TemplateRow::into_template).collect()
    }

    async fn get(&self, id: TemplateId) -> Result<NodeTemplate, EmberTroveError> {
        sqlx::query_as::<_, TemplateRow>(&format!(
            "SELECT {COLS} FROM node_templates WHERE id = $1",
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get template failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound("template not found".to_string()))?
        .into_template()
    }

    async fn create(
        &self,
        created_by: &str,
        req: CreateTemplateRequest,
    ) -> Result<NodeTemplate, EmberTroveError> {
        let row = sqlx::query_as::<_, TemplateRow>(&format!(
            "INSERT INTO node_templates (name, description, node_type, body, created_by) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING {COLS}",
        ))
        .bind(&req.name)
        .bind(req.description.as_deref())
        .bind(node_type_str(&req.node_type))
        .bind(&req.body)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("create template failed: {e}")))?;

        row.into_template()
    }

    async fn update(
        &self,
        id: TemplateId,
        req: UpdateTemplateRequest,
    ) -> Result<NodeTemplate, EmberTroveError> {
        let row = sqlx::query_as::<_, TemplateRow>(&format!(
            "UPDATE node_templates \
             SET name = $1, description = $2, node_type = $3, body = $4, updated_at = now() \
             WHERE id = $5 \
             RETURNING {COLS}",
        ))
        .bind(&req.name)
        .bind(req.description.as_deref())
        .bind(node_type_str(&req.node_type))
        .bind(&req.body)
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("update template failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound("template not found".to_string()))?;

        row.into_template()
    }

    async fn delete(&self, id: TemplateId) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM node_templates WHERE id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete template failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound("template not found".to_string()));
        }
        Ok(())
    }

    async fn set_default(
        &self,
        id: TemplateId,
        created_by: &str,
    ) -> Result<NodeTemplate, EmberTroveError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("begin tx failed: {e}")))?;

        // Step 1 — clear all other defaults for the same owner + node_type.
        // The subquery fetches the type of the target template so we don't need
        // a separate round-trip.
        sqlx::query(
            "UPDATE node_templates \
             SET is_default = FALSE \
             WHERE created_by = $1 \
               AND node_type = (SELECT node_type FROM node_templates WHERE id = $2) \
               AND id != $2",
        )
        .bind(created_by)
        .bind(id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("clear defaults failed: {e}")))?;

        // Step 2 — toggle the target template's own flag.
        let row = sqlx::query_as::<_, TemplateRow>(&format!(
            "UPDATE node_templates \
             SET is_default = NOT is_default, updated_at = now() \
             WHERE id = $1 AND created_by = $2 \
             RETURNING {COLS}",
        ))
        .bind(id.0)
        .bind(created_by)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("set_default toggle failed: {e}")))?
        .ok_or_else(|| {
            EmberTroveError::NotFound(
                "template not found or not owned by current user".to_string(),
            )
        })?;

        tx.commit()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("commit tx failed: {e}")))?;

        row.into_template()
    }
}
