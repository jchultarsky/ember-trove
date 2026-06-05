//! Server-side PKCE verifier store.
//!
//! The OAuth `state` parameter travels through the Cognito redirect URL and is
//! used to look up the matching PKCE `code_verifier` at /api/auth/callback.
//!
//! This was previously an in-memory `Arc<Mutex<HashMap>>`, which was wiped on
//! every API restart — so any user mid-login during a deploy hit
//! `invalid_code_verifier`. Persisting to Postgres makes in-flight logins
//! survive restarts. Entries are consumed exactly once and expire after
//! [`PKCE_TTL`]; a background sweeper purges stragglers.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::EmberTroveError;
use sqlx::PgPool;

/// Maximum age of a pending PKCE entry. Entries older than this are treated as
/// expired at callback and purged by the sweeper.
pub const PKCE_TTL: Duration = Duration::from_secs(600); // 10 minutes

#[async_trait]
pub trait PkceRepo: Send + Sync {
    /// Persist a verifier keyed by the OAuth `state`. Overwrites any existing
    /// row for the same state (the state is a fresh 16-byte random token, so a
    /// collision is astronomically unlikely — the upsert is purely defensive).
    async fn store(&self, state: &str, code_verifier: &str) -> Result<(), EmberTroveError>;

    /// Atomically consume the verifier for `state`: the row is always deleted
    /// (preventing replay), and the verifier is returned only when the entry is
    /// younger than `ttl`. Absent or expired → `Ok(None)`.
    async fn take(&self, state: &str, ttl: Duration) -> Result<Option<String>, EmberTroveError>;

    /// Delete entries older than `ttl`. Returns the number of rows removed.
    async fn sweep_expired(&self, ttl: Duration) -> Result<u64, EmberTroveError>;
}

pub struct PgPkceRepo {
    pool: PgPool,
}

impl PgPkceRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct PkceRow {
    code_verifier: String,
    created_at: DateTime<Utc>,
}

/// Pure freshness check, factored out of [`PkceRepo::take`] so it can be unit
/// tested without a database. Returns the verifier iff `created_at` is no older
/// than `ttl` relative to `now` (clock skew producing a future timestamp counts
/// as fresh).
fn verifier_if_fresh(
    code_verifier: String,
    created_at: DateTime<Utc>,
    now: DateTime<Utc>,
    ttl: Duration,
) -> Option<String> {
    let ttl = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::seconds(600));
    if now.signed_duration_since(created_at) < ttl {
        Some(code_verifier)
    } else {
        None
    }
}

/// Cutoff timestamp for the sweep, computed in Rust to avoid binding a Postgres
/// `interval` from a `Duration`.
fn expiry_cutoff(now: DateTime<Utc>, ttl: Duration) -> DateTime<Utc> {
    let ttl = chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::seconds(600));
    now - ttl
}

#[async_trait]
impl PkceRepo for PgPkceRepo {
    async fn store(&self, state: &str, code_verifier: &str) -> Result<(), EmberTroveError> {
        sqlx::query(
            r#"
            INSERT INTO pkce_verifiers (state, code_verifier)
            VALUES ($1, $2)
            ON CONFLICT (state)
            DO UPDATE SET code_verifier = EXCLUDED.code_verifier, created_at = now()
            "#,
        )
        .bind(state)
        .bind(code_verifier)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("store pkce verifier failed: {e}")))?;
        Ok(())
    }

    async fn take(&self, state: &str, ttl: Duration) -> Result<Option<String>, EmberTroveError> {
        // Delete unconditionally (consume-once / anti-replay), then validate
        // freshness in Rust against the returned timestamp.
        let row = sqlx::query_as::<_, PkceRow>(
            "DELETE FROM pkce_verifiers WHERE state = $1 RETURNING code_verifier, created_at",
        )
        .bind(state)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("take pkce verifier failed: {e}")))?;

        Ok(row.and_then(|r| verifier_if_fresh(r.code_verifier, r.created_at, Utc::now(), ttl)))
    }

    async fn sweep_expired(&self, ttl: Duration) -> Result<u64, EmberTroveError> {
        let cutoff = expiry_cutoff(Utc::now(), ttl);
        let result = sqlx::query("DELETE FROM pkce_verifiers WHERE created_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("sweep pkce verifiers failed: {e}")))?;
        Ok(result.rows_affected())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_entry_returns_verifier() {
        let now = Utc::now();
        let created = now - chrono::Duration::seconds(60);
        assert_eq!(
            verifier_if_fresh("v".to_string(), created, now, Duration::from_secs(600)),
            Some("v".to_string())
        );
    }

    #[test]
    fn expired_entry_returns_none() {
        let now = Utc::now();
        let created = now - chrono::Duration::seconds(601);
        assert_eq!(
            verifier_if_fresh("v".to_string(), created, now, Duration::from_secs(600)),
            None
        );
    }

    #[test]
    fn future_timestamp_from_clock_skew_counts_as_fresh() {
        let now = Utc::now();
        let created = now + chrono::Duration::seconds(5);
        assert_eq!(
            verifier_if_fresh("v".to_string(), created, now, Duration::from_secs(600)),
            Some("v".to_string())
        );
    }

    #[test]
    fn cutoff_is_ttl_before_now() {
        let now = Utc::now();
        let cutoff = expiry_cutoff(now, Duration::from_secs(600));
        assert_eq!(
            now.signed_duration_since(cutoff),
            chrono::Duration::seconds(600)
        );
    }
}
