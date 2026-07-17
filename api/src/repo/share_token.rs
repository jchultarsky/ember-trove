use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, ShareTokenId},
    share_token::{CreateShareTokenRequest, ShareToken},
};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait ShareTokenRepo: Send + Sync {
    async fn list_all(&self) -> Result<Vec<ShareToken>, EmberTroveError>;

    async fn create(
        &self,
        node_id: NodeId,
        created_by: &str,
        req: &CreateShareTokenRequest,
    ) -> Result<ShareToken, EmberTroveError>;

    async fn list(&self, node_id: NodeId) -> Result<Vec<ShareToken>, EmberTroveError>;

    async fn find_by_token(&self, token: Uuid) -> Result<Option<ShareToken>, EmberTroveError>;

    /// Revoke a token, scoped to its owning node: the DELETE matches both the
    /// token id and `node_id`, so a token id from another node is NotFound
    /// rather than revoked (the caller has only proven ownership of `node_id`).
    async fn revoke(&self, id: ShareTokenId, node_id: NodeId) -> Result<(), EmberTroveError>;
}

pub struct PgShareTokenRepo {
    pool: PgPool,
}

impl PgShareTokenRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct ShareTokenRow {
    id: Uuid,
    node_id: Uuid,
    token: Uuid,
    created_by: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}

impl ShareTokenRow {
    fn into_share_token(self) -> ShareToken {
        ShareToken {
            id: ShareTokenId(self.id),
            node_id: NodeId(self.node_id),
            token: self.token,
            created_by: self.created_by,
            created_at: self.created_at,
            expires_at: self.expires_at,
        }
    }
}

#[async_trait]
impl ShareTokenRepo for PgShareTokenRepo {
    async fn list_all(&self) -> Result<Vec<ShareToken>, EmberTroveError> {
        let rows = sqlx::query_as::<_, ShareTokenRow>(
            "SELECT id, node_id, token, created_by, created_at, expires_at
             FROM share_tokens ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all share_tokens failed: {e}")))?;

        Ok(rows.into_iter().map(|r| r.into_share_token()).collect())
    }

    async fn create(
        &self,
        node_id: NodeId,
        created_by: &str,
        req: &CreateShareTokenRequest,
    ) -> Result<ShareToken, EmberTroveError> {
        let row = sqlx::query_as::<_, ShareTokenRow>(
            r#"
            INSERT INTO share_tokens (node_id, created_by, expires_at)
            VALUES ($1, $2, $3)
            RETURNING id, node_id, token, created_by, created_at, expires_at
            "#,
        )
        .bind(node_id.inner())
        .bind(created_by)
        .bind(req.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("create share token failed: {e}")))?;

        Ok(row.into_share_token())
    }

    async fn list(&self, node_id: NodeId) -> Result<Vec<ShareToken>, EmberTroveError> {
        let rows = sqlx::query_as::<_, ShareTokenRow>(
            r#"
            SELECT id, node_id, token, created_by, created_at, expires_at
            FROM share_tokens
            WHERE node_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(node_id.inner())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list share tokens failed: {e}")))?;

        Ok(rows.into_iter().map(|r| r.into_share_token()).collect())
    }

    async fn find_by_token(&self, token: Uuid) -> Result<Option<ShareToken>, EmberTroveError> {
        let row = sqlx::query_as::<_, ShareTokenRow>(
            r#"
            SELECT id, node_id, token, created_by, created_at, expires_at
            FROM share_tokens
            WHERE token = $1
              AND (expires_at IS NULL OR expires_at > now())
            "#,
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("find share token failed: {e}")))?;

        Ok(row.map(|r| r.into_share_token()))
    }

    async fn revoke(&self, id: ShareTokenId, node_id: NodeId) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM share_tokens WHERE id = $1 AND node_id = $2")
            .bind(id.inner())
            .bind(node_id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("revoke share token failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!(
                "share token {id} not found"
            )));
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_token_row_into_share_token_maps_fields() {
        let row = ShareTokenRow {
            id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            token: Uuid::new_v4(),
            created_by: "user-sub".to_string(),
            created_at: Utc::now(),
            expires_at: None,
        };
        let st = row.into_share_token();
        assert_eq!(st.created_by, "user-sub");
        assert!(st.expires_at.is_none());
    }
}
