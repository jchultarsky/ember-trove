use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, PermissionId},
    permission::{GrantPermissionRequest, Permission, PermissionRole},
};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait PermissionRepo: Send + Sync {
    async fn grant(
        &self,
        node_id: NodeId,
        granted_by: &str,
        req: GrantPermissionRequest,
    ) -> Result<Permission, EmberTroveError>;

    async fn revoke(&self, id: PermissionId) -> Result<(), EmberTroveError>;

    async fn list(&self, node_id: NodeId) -> Result<Vec<Permission>, EmberTroveError>;

    async fn find(
        &self,
        node_id: NodeId,
        subject_id: &str,
    ) -> Result<Option<Permission>, EmberTroveError>;

    /// List all permissions, optionally filtered to a specific node.
    async fn list_all(&self, node_id: Option<NodeId>) -> Result<Vec<Permission>, EmberTroveError>;

    /// Look up a single permission grant by its ID.
    async fn find_by_id(&self, id: PermissionId) -> Result<Option<Permission>, EmberTroveError>;

    /// Update the role on an existing permission by its ID.
    async fn update(
        &self,
        id: PermissionId,
        role: PermissionRole,
        updated_by: &str,
    ) -> Result<Permission, EmberTroveError>;
}

pub struct PgPermissionRepo {
    pool: PgPool,
}

impl PgPermissionRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct PermissionRow {
    id: Uuid,
    node_id: Uuid,
    subject_id: String,
    role: String,
    granted_by: String,
    created_at: DateTime<Utc>,
}

pub(crate) fn parse_role(s: &str) -> Result<PermissionRole, EmberTroveError> {
    match s {
        "owner" => Ok(PermissionRole::Owner),
        "editor" => Ok(PermissionRole::Editor),
        "viewer" => Ok(PermissionRole::Viewer),
        other => Err(EmberTroveError::Internal(format!(
            "unknown permission_role: {other}"
        ))),
    }
}

pub(crate) fn role_to_str(role: &PermissionRole) -> &'static str {
    match role {
        PermissionRole::Owner => "owner",
        PermissionRole::Editor => "editor",
        PermissionRole::Viewer => "viewer",
    }
}

impl PermissionRow {
    fn into_permission(self) -> Result<Permission, EmberTroveError> {
        Ok(Permission {
            id: PermissionId(self.id),
            node_id: NodeId(self.node_id),
            subject_id: self.subject_id,
            role: parse_role(&self.role)?,
            granted_by: self.granted_by,
            created_at: self.created_at,
        })
    }
}

#[async_trait]
impl PermissionRepo for PgPermissionRepo {
    async fn grant(
        &self,
        node_id: NodeId,
        granted_by: &str,
        req: GrantPermissionRequest,
    ) -> Result<Permission, EmberTroveError> {
        let role_str = role_to_str(&req.role);
        let row = sqlx::query_as::<_, PermissionRow>(
            r#"
            INSERT INTO permissions (node_id, subject_id, role, granted_by)
            VALUES ($1, $2, $3::permission_role, $4)
            ON CONFLICT (node_id, subject_id)
                DO UPDATE SET role = EXCLUDED.role, granted_by = EXCLUDED.granted_by
            RETURNING id, node_id, subject_id, role::text, granted_by, created_at
            "#,
        )
        .bind(node_id.inner())
        .bind(&req.subject_id)
        .bind(role_str)
        .bind(granted_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("grant permission failed: {e}")))?;

        row.into_permission()
    }

    async fn revoke(&self, id: PermissionId) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM permissions WHERE id = $1")
            .bind(id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("revoke permission failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!(
                "permission {id} not found"
            )));
        }
        Ok(())
    }

    async fn list(&self, node_id: NodeId) -> Result<Vec<Permission>, EmberTroveError> {
        let rows = sqlx::query_as::<_, PermissionRow>(
            r#"
            SELECT id, node_id, subject_id, role::text, granted_by, created_at
            FROM permissions
            WHERE node_id = $1
            ORDER BY created_at
            "#,
        )
        .bind(node_id.inner())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list permissions failed: {e}")))?;

        rows.into_iter().map(|r| r.into_permission()).collect()
    }

    async fn find(
        &self,
        node_id: NodeId,
        subject_id: &str,
    ) -> Result<Option<Permission>, EmberTroveError> {
        let row = sqlx::query_as::<_, PermissionRow>(
            r#"
            SELECT id, node_id, subject_id, role::text, granted_by, created_at
            FROM permissions
            WHERE node_id = $1 AND subject_id = $2
            "#,
        )
        .bind(node_id.inner())
        .bind(subject_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("find permission failed: {e}")))?;

        row.map(|r| r.into_permission()).transpose()
    }

    async fn list_all(&self, node_id: Option<NodeId>) -> Result<Vec<Permission>, EmberTroveError> {
        let rows = sqlx::query_as::<_, PermissionRow>(
            r#"
            SELECT id, node_id, subject_id, role::text, granted_by, created_at
            FROM permissions
            WHERE ($1::uuid IS NULL OR node_id = $1)
            ORDER BY created_at
            "#,
        )
        .bind(node_id.map(|n| n.inner()))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all permissions failed: {e}")))?;

        rows.into_iter().map(|r| r.into_permission()).collect()
    }

    async fn find_by_id(&self, id: PermissionId) -> Result<Option<Permission>, EmberTroveError> {
        let row = sqlx::query_as::<_, PermissionRow>(
            r#"
            SELECT id, node_id, subject_id, role::text, granted_by, created_at
            FROM permissions
            WHERE id = $1
            "#,
        )
        .bind(id.inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("find_by_id permission failed: {e}")))?;

        row.map(|r| r.into_permission()).transpose()
    }

    async fn update(
        &self,
        id: PermissionId,
        role: PermissionRole,
        updated_by: &str,
    ) -> Result<Permission, EmberTroveError> {
        let role_str = role_to_str(&role);
        let row = sqlx::query_as::<_, PermissionRow>(
            r#"
            UPDATE permissions
               SET role = $2::permission_role, granted_by = $3
             WHERE id = $1
            RETURNING id, node_id, subject_id, role::text, granted_by, created_at
            "#,
        )
        .bind(id.inner())
        .bind(role_str)
        .bind(updated_by)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("update permission failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("permission {id} not found")))?;

        row.into_permission()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_role_known_variants() {
        assert_eq!(parse_role("owner").unwrap(), PermissionRole::Owner);
        assert_eq!(parse_role("editor").unwrap(), PermissionRole::Editor);
        assert_eq!(parse_role("viewer").unwrap(), PermissionRole::Viewer);
    }

    #[test]
    fn parse_role_unknown_returns_error() {
        assert!(parse_role("superuser").is_err());
        assert!(parse_role("").is_err());
        assert!(parse_role("Owner").is_err()); // case-sensitive
    }

    #[test]
    fn role_to_str_and_back_round_trip() {
        for role in [
            PermissionRole::Owner,
            PermissionRole::Editor,
            PermissionRole::Viewer,
        ] {
            let s = role_to_str(&role);
            let back = parse_role(s).expect("round-trip should succeed");
            assert_eq!(back, role);
        }
    }

    #[test]
    fn role_to_str_produces_lowercase() {
        assert_eq!(role_to_str(&PermissionRole::Owner), "owner");
        assert_eq!(role_to_str(&PermissionRole::Editor), "editor");
        assert_eq!(role_to_str(&PermissionRole::Viewer), "viewer");
    }
}
