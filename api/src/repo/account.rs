//! Account lifecycle (#239): the suspended-account list.

use std::collections::HashSet;

use async_trait::async_trait;
use common::EmberTroveError;
use sqlx::PgPool;

#[async_trait]
pub trait AccountRepo: Send + Sync {
    /// Every suspended subject.
    async fn suspended(&self) -> Result<HashSet<String>, EmberTroveError>;

    /// Record the subject as suspended by `by`. Suspending an account
    /// that is already suspended keeps the first record.
    async fn suspend(&self, subject_id: &str, by: &str) -> Result<(), EmberTroveError>;

    /// Remove the subject from the list. Missing is fine.
    async fn unsuspend(&self, subject_id: &str) -> Result<(), EmberTroveError>;
}

pub struct PgAccountRepo {
    pool: PgPool,
}

impl PgAccountRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AccountRepo for PgAccountRepo {
    async fn suspended(&self) -> Result<HashSet<String>, EmberTroveError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT subject_id FROM suspended_accounts")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                EmberTroveError::Internal(format!("list suspended accounts failed: {e}"))
            })?;
        Ok(rows.into_iter().collect())
    }

    async fn suspend(&self, subject_id: &str, by: &str) -> Result<(), EmberTroveError> {
        sqlx::query(
            "INSERT INTO suspended_accounts (subject_id, suspended_by) VALUES ($1, $2)
             ON CONFLICT (subject_id) DO NOTHING",
        )
        .bind(subject_id)
        .bind(by)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("suspend account failed: {e}")))?;
        Ok(())
    }

    async fn unsuspend(&self, subject_id: &str) -> Result<(), EmberTroveError> {
        sqlx::query("DELETE FROM suspended_accounts WHERE subject_id = $1")
            .bind(subject_id)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("unsuspend account failed: {e}")))?;
        Ok(())
    }
}
