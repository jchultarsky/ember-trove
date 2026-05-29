//! Operational metrics endpoint — admin-only, no auth JWT required beyond role check.
//!
//! `GET /api/metrics` returns a JSON snapshot suitable for CloudWatch dashboards,
//! alerting, or basic health checks.  Fields:
//!
//! - `uptime_secs`        — process uptime in seconds
//! - `db.pool_size`       — current number of open connections (idle + in-use)
//! - `db.pool_idle`       — idle connections waiting to be acquired
//! - `counts.*`           — row counts for each core table
//! - `requests.*`         — process-lifetime HTTP request counters by status class
//! - `version`            — value of the `CARGO_PKG_VERSION` build-time env var

use std::sync::atomic::{AtomicU64, Ordering};

use axum::{extract::{Request, State}, middleware::Next, response::Response, Extension, Json};
use common::auth::AuthClaims;
use serde::Serialize;

use crate::{auth::permissions::require_admin, error::ApiError, state::AppState};

pub fn router() -> axum::Router<AppState> {
    use axum::routing::get;
    axum::Router::new().route("/", get(get_metrics))
}

// ── Request counters ────────────────────────────────────────────────────────────

/// Process-lifetime HTTP request counters, bucketed by response status class.
/// Lives in [`AppState`] behind an `Arc`; updated by [`track_requests`] and read
/// by the metrics endpoint. All counters are monotonic and reset on restart.
#[derive(Default)]
pub struct RequestMetrics {
    total: AtomicU64,
    status_2xx: AtomicU64,
    status_3xx: AtomicU64,
    status_4xx: AtomicU64,
    status_5xx: AtomicU64,
}

impl RequestMetrics {
    /// Record one completed response by its HTTP status code.
    pub fn record(&self, status: u16) {
        self.total.fetch_add(1, Ordering::Relaxed);
        let bucket = match status {
            200..=299 => &self.status_2xx,
            300..=399 => &self.status_3xx,
            400..=499 => &self.status_4xx,
            _ => &self.status_5xx,
        };
        bucket.fetch_add(1, Ordering::Relaxed);
    }

    /// Read the current counter values as a serializable snapshot.
    pub fn snapshot(&self) -> RequestCounts {
        RequestCounts {
            total: self.total.load(Ordering::Relaxed),
            status_2xx: self.status_2xx.load(Ordering::Relaxed),
            status_3xx: self.status_3xx.load(Ordering::Relaxed),
            status_4xx: self.status_4xx.load(Ordering::Relaxed),
            status_5xx: self.status_5xx.load(Ordering::Relaxed),
        }
    }
}

/// Axum middleware that counts every response by status class. Applied at the
/// outermost router layer so health checks and rate-limited routes are included.
pub async fn track_requests(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let response = next.run(req).await;
    state.request_metrics.record(response.status().as_u16());
    response
}

// ── Response shape ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Metrics {
    pub version: &'static str,
    pub uptime_secs: u64,
    pub db: DbMetrics,
    pub counts: EntityCounts,
    pub requests: RequestCounts,
}

#[derive(Serialize)]
pub struct RequestCounts {
    pub total: u64,
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
}

#[derive(Serialize)]
pub struct DbMetrics {
    pub pool_size: u32,
    pub pool_idle: u32,
}

#[derive(Serialize)]
pub struct EntityCounts {
    pub nodes: i64,
    pub edges: i64,
    pub tags: i64,
    pub notes: i64,
    pub tasks: i64,
    pub attachments: i64,
    pub favorites: i64,
}

// ── Handler ───────────────────────────────────────────────────────────────────

async fn get_metrics(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Metrics>, ApiError> {
    require_admin(&claims)?;

    let uptime_secs = state.started_at.elapsed().as_secs();

    let pool_size = state.pool.size();
    let pool_idle = state.pool.num_idle() as u32;

    // Run all count queries concurrently.
    let (nodes, edges, tags, notes, tasks, attachments, favorites) = tokio::try_join!(
        count(&state.pool, "nodes"),
        count(&state.pool, "edges"),
        count(&state.pool, "tags"),
        count(&state.pool, "notes"),
        count(&state.pool, "tasks"),
        count(&state.pool, "attachments"),
        count(&state.pool, "user_favorites"),
    )?;

    Ok(Json(Metrics {
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs,
        db: DbMetrics {
            pool_size,
            pool_idle,
        },
        counts: EntityCounts {
            nodes,
            edges,
            tags,
            notes,
            tasks,
            attachments,
            favorites,
        },
        requests: state.request_metrics.snapshot(),
    }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn count(pool: &sqlx::PgPool, table: &str) -> Result<i64, ApiError> {
    // Table name comes from compile-time string literals — no injection risk.
    let sql = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .map_err(|e| ApiError::Internal(format!("metrics count({table}) failed: {e}")))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_buckets_by_status_class() {
        let m = RequestMetrics::default();
        for s in [200u16, 201, 204, 301, 404, 401, 500, 502, 503] {
            m.record(s);
        }
        let snap = m.snapshot();
        assert_eq!(snap.total, 9);
        assert_eq!(snap.status_2xx, 3);
        assert_eq!(snap.status_3xx, 1);
        assert_eq!(snap.status_4xx, 2);
        assert_eq!(snap.status_5xx, 3);
    }
}
