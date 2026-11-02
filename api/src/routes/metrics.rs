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
//! - `routes`             — latency per route since start (P0 of the performance
//!   phase): count, mean, p50/p95/p99 (bucket bounds) and max, slowest total first
//! - `version`            — value of the `CARGO_PKG_VERSION` build-time env var
//!
//! A route is its matched pattern (`GET /api/nodes/{id}`), never the request's
//! path or query, so no ids or search terms are kept.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::{
    Extension, Json,
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
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
    /// Latency per route, keyed `"{method} {matched pattern}"`. Bounded by the
    /// number of routes, since a key is never built from the request's path.
    routes: Mutex<HashMap<String, LatencyHistogram>>,
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

    /// Record one response's latency under its route.
    pub fn record_route(&self, route: String, micros: u64) {
        // A poisoned lock only means another request panicked mid-update;
        // skipping one sample is better than failing the response.
        if let Ok(mut routes) = self.routes.lock() {
            routes.entry(route).or_default().record(micros);
        }
    }

    /// Latency per route, the largest total time first.
    pub fn routes_snapshot(&self) -> Vec<RouteLatency> {
        let Ok(routes) = self.routes.lock() else {
            return Vec::new();
        };
        let mut out: Vec<(u64, RouteLatency)> = routes
            .iter()
            .map(|(route, h)| (h.sum_us, h.summary(route)))
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.route.cmp(&b.1.route)));
        out.into_iter().map(|(_, r)| r).collect()
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
    let route = route_label(req.method().as_str(), req.extensions().get::<MatchedPath>());
    let started = Instant::now();
    let response = next.run(req).await;
    let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    state.request_metrics.record(response.status().as_u16());
    state.request_metrics.record_route(route, micros);
    response
}

/// The label a request is counted and logged under: its method and matched
/// route pattern. Both come from fixed sets, so no request can mint a new
/// label: unmatched paths share one, and so does every method outside the
/// standard ones (http accepts any token as a method, and a 405 is still
/// counted under its route; review of P0).
pub fn route_label(method: &str, matched: Option<&MatchedPath>) -> String {
    let method = match method {
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" => method,
        _ => "OTHER",
    };
    format!(
        "{method} {}",
        matched.map_or("(unmatched)", MatchedPath::as_str)
    )
}

// ── Latency histogram ─────────────────────────────────────────────────────────

/// Upper bounds of the latency buckets, in µs; the last takes the rest.
const BUCKETS_US: [u64; 16] = [
    250,
    500,
    1_000,
    2_000,
    5_000,
    10_000,
    20_000,
    50_000,
    100_000,
    200_000,
    500_000,
    1_000_000,
    2_000_000,
    5_000_000,
    10_000_000,
    u64::MAX,
];

/// A fixed-bucket latency histogram: constant memory per route, and
/// percentiles good to one bucket (each reported as its bucket's upper
/// bound, never above the exact maximum).
#[derive(Debug, Default, Clone)]
pub struct LatencyHistogram {
    buckets: [u64; BUCKETS_US.len()],
    count: u64,
    sum_us: u64,
    max_us: u64,
}

impl LatencyHistogram {
    pub fn record(&mut self, micros: u64) {
        let i = BUCKETS_US
            .iter()
            .position(|&bound| micros <= bound)
            .unwrap_or(BUCKETS_US.len() - 1);
        if let Some(b) = self.buckets.get_mut(i) {
            *b += 1;
        }
        self.count += 1;
        self.sum_us = self.sum_us.saturating_add(micros);
        self.max_us = self.max_us.max(micros);
    }

    /// The latency that a share `q` (0..=1) of the samples stay within: the
    /// upper bound of the bucket holding that rank, capped at the maximum.
    /// `0` with no samples.
    pub fn percentile_us(&self, q: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        // The rank (1-based) of the sample at quantile q.
        let rank = ((q.clamp(0.0, 1.0) * self.count as f64).ceil() as u64).max(1);
        let mut seen = 0;
        for (n, bound) in self.buckets.iter().zip(BUCKETS_US) {
            seen += n;
            if seen >= rank {
                return bound.min(self.max_us);
            }
        }
        self.max_us
    }

    fn summary(&self, route: &str) -> RouteLatency {
        let ms = |us: u64| (us as f64 / 100.0).round() / 10.0;
        RouteLatency {
            route: route.to_string(),
            count: self.count,
            mean_ms: self.sum_us.checked_div(self.count).map_or(0.0, ms),
            p50_ms: ms(self.percentile_us(0.50)),
            p95_ms: ms(self.percentile_us(0.95)),
            p99_ms: ms(self.percentile_us(0.99)),
            max_ms: ms(self.max_us),
        }
    }
}

// ── Response shape ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Metrics {
    pub version: &'static str,
    pub uptime_secs: u64,
    pub db: DbMetrics,
    pub counts: EntityCounts,
    pub requests: RequestCounts,
    pub routes: Vec<RouteLatency>,
}

/// One route's latency since start, in ms (0.1 ms resolution).
#[derive(Debug, Serialize, PartialEq)]
pub struct RouteLatency {
    pub route: String,
    pub count: u64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
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
        // The tables are node_notes / node_tasks; "notes"/"tasks" don't exist,
        // so this endpoint used to 500 on every call.
        count(&state.pool, "node_notes"),
        count(&state.pool, "node_tasks"),
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
        routes: state.request_metrics.routes_snapshot(),
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

    #[test]
    fn an_empty_histogram_reports_zero() {
        let h = LatencyHistogram::default();
        assert_eq!(h.percentile_us(0.5), 0);
        assert_eq!(h.percentile_us(0.99), 0);
    }

    #[test]
    fn percentiles_are_bucket_bounds_capped_at_the_max() {
        let mut h = LatencyHistogram::default();
        // 90 fast (300 µs → the 500 µs bucket), 9 at 30 ms, 1 at 1.2 s.
        for _ in 0..90 {
            h.record(300);
        }
        for _ in 0..9 {
            h.record(30_000);
        }
        h.record(1_200_000);
        assert_eq!(h.percentile_us(0.50), 500);
        assert_eq!(h.percentile_us(0.90), 500, "the 90th sample is still fast");
        assert_eq!(h.percentile_us(0.95), 50_000);
        assert_eq!(h.percentile_us(0.99), 50_000);
        assert_eq!(
            h.percentile_us(1.0),
            1_200_000,
            "capped at the real max, not 2 s"
        );
        let s = h.summary("GET /x");
        assert_eq!((s.count, s.max_ms, s.p50_ms), (100, 1200.0, 0.5));
    }

    #[test]
    fn a_huge_sample_lands_in_the_last_bucket() {
        let mut h = LatencyHistogram::default();
        h.record(u64::MAX);
        h.record(60_000_000);
        assert_eq!(h.count, 2);
        // Both sit in the open top bucket (over 10 s), which has no finer
        // resolution: any percentile there reports the maximum.
        assert_eq!(h.percentile_us(0.5), u64::MAX);
        assert_eq!(h.max_us, u64::MAX);
    }

    #[test]
    fn routes_are_listed_by_total_time() {
        let m = RequestMetrics::default();
        m.record_route("GET /a".into(), 1_000);
        m.record_route("GET /b".into(), 50_000);
        m.record_route("GET /a".into(), 1_000);
        let routes = m.routes_snapshot();
        assert_eq!(
            routes
                .iter()
                .map(|r| (r.route.as_str(), r.count))
                .collect::<Vec<_>>(),
            [("GET /b", 1), ("GET /a", 2)]
        );
    }

    #[test]
    fn an_unmatched_request_shares_one_label() {
        assert_eq!(route_label("GET", None), "GET (unmatched)");
    }

    #[test]
    fn a_made_up_method_shares_one_label() {
        let long = "X".repeat(5000);
        for m in ["FOO123", "PROPFIND", "get", long.as_str()] {
            assert_eq!(route_label(m, None), "OTHER (unmatched)");
        }
        assert_eq!(route_label("DELETE", None), "DELETE (unmatched)");
    }
}
