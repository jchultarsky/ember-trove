pub mod admin;
pub mod attachments;
pub mod auth;
pub mod backup;
pub mod edges;
pub mod editor_prefs;
pub mod export;
pub mod favorites;
pub mod graph;
pub mod inbox;
pub mod metrics;
pub mod node_links;
pub mod nodes;
pub mod notes;
pub mod permissions;
pub mod search;
pub mod search_presets;
pub mod share;
pub mod tags;
pub mod tasks;
pub mod templates;
pub mod webhooks;

use axum::http::{Method, StatusCode, header};
use axum::{Json, Router, extract::State, middleware, routing::get};
use chrono::Utc;
use serde_json::{Value, json};
use std::time::Duration;
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::LatencyUnit;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{auth::middleware::require_auth, state::AppState};

/// Per-IP budget for the write (mutating-method) rate limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RateLimitBudget {
    /// Milliseconds to replenish one token.
    replenish_ms: u64,
    /// Bucket size — the largest burst allowed before throttling.
    burst: u32,
}

/// Production budget: 10 req/s sustained, burst of 100.
const PROD_RATE_LIMIT: RateLimitBudget = RateLimitBudget {
    replenish_ms: 100,
    burst: 100,
};

/// Playwright stack only. The whole suite shares one client IP and writes its
/// fixtures through the API, so on a fast runner it outruns the production
/// budget and specs fail on HTTP 429 (2026-10-16, flaky on develop CI).
const E2E_RATE_LIMIT: RateLimitBudget = RateLimitBudget {
    replenish_ms: 1,
    burst: 10_000,
};

/// Choose the write rate-limit budget.
///
/// SECURITY: the e2e budget needs BOTH the `e2e-bypass` cargo feature (which no
/// release image is built with — the caller passes `cfg!`, a compile-time
/// constant) AND `E2E_AUTH_BYPASS=1` at runtime: the same double gate as the
/// auth bypass in `auth::middleware`. Everything else gets `PROD_RATE_LIMIT`.
fn rate_limit_budget(e2e_bypass_compiled: bool, bypass_env: Option<&str>) -> RateLimitBudget {
    if e2e_bypass_compiled && bypass_env == Some("1") {
        E2E_RATE_LIMIT
    } else {
        PROD_RATE_LIMIT
    }
}

pub fn build_router(state: AppState) -> anyhow::Result<Router> {
    // --- Rate limiting -------------------------------------------------------
    // PROD_RATE_LIMIT per peer IP (see rate_limit_budget for the e2e case).
    // Only applies to mutating methods (POST/PUT/DELETE/PATCH) to keep read
    // traffic snappy for a single-user PKM.
    // SmartIpKeyExtractor reads X-Real-IP / X-Forwarded-For (set by nginx) and
    // falls back to ConnectInfo<SocketAddr> for direct connections.
    let budget = rate_limit_budget(
        cfg!(feature = "e2e-bypass"),
        std::env::var("E2E_AUTH_BYPASS").ok().as_deref(),
    );
    if budget != PROD_RATE_LIMIT {
        tracing::warn!(
            ?budget,
            "e2e bypass armed: write rate limit relaxed (NEVER in production)"
        );
    }
    let governor_conf = GovernorConfigBuilder::default()
        .key_extractor(SmartIpKeyExtractor)
        .per_millisecond(budget.replenish_ms)
        .burst_size(budget.burst)
        .finish()
        .ok_or_else(|| anyhow::anyhow!("invalid rate-limit governor config"))?;
    let governor_limiter = governor_conf.limiter().clone();
    // Spawn background task to evict stale IP entries from the limiter's map.
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            governor_limiter.retain_recent();
        }
    });
    let rate_limit = GovernorLayer::new(std::sync::Arc::new(governor_conf));

    // --- CORS ----------------------------------------------------------------
    // SECURITY: Fail hard if FRONTEND_URL is not a valid header value.
    // Silently falling back to AllowOrigin::any() with credentials would be an
    // open CORS policy — any origin could make authenticated cross-site requests.
    let origin = AllowOrigin::exact(state.auth.frontend_url.parse().map_err(|e| {
        anyhow::anyhow!(
            "FRONTEND_URL '{}' is not a valid CORS origin: {e}",
            state.auth.frontend_url
        )
    })?);

    // With allow_credentials(true), headers and methods must be explicit (not Any).
    let cors = CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
            header::COOKIE,
        ])
        // Expose X-Request-Id so clients can correlate logs with server traces.
        .expose_headers([header::HeaderName::from_static("x-request-id")])
        .allow_credentials(true);

    // Protected routes — require a valid JWT.
    let protected = Router::new()
        .merge(auth::protected_router())
        .nest("/nodes", nodes::router())
        .nest("/nodes/{node_id}/tasks", tasks::node_task_router())
        .nest("/tasks", tasks::task_router())
        .nest("/my-day", tasks::my_day_router())
        .nest("/calendar", tasks::calendar_router())
        .nest("/dashboard/projects", tasks::dashboard_router())
        .nest("/dashboard/activity", tasks::dashboard_activity_router())
        .nest("/inbox", inbox::router())
        .nest("/nodes/{node_id}/notes", notes::node_note_router())
        .nest("/notes", notes::note_router())
        .nest("/edges", edges::router())
        .nest("/tags", tags::router())
        .nest("/attachments", attachments::router())
        .nest("/search", search::router())
        .nest("/graph", graph::router())
        .nest("/permissions", permissions::router())
        .nest("/favorites", favorites::router())
        .nest("/nodes/{node_id}/share", share::node_share_router())
        .nest("/templates", templates::router())
        .nest("/search-presets", search_presets::router())
        .nest("/nodes/{node_id}/links", node_links::router())
        .nest("/webhooks", webhooks::router())
        .nest("/export", export::router())
        .nest("/admin", admin::router())
        .nest("/admin/backups", backup::router())
        .nest("/metrics", metrics::router())
        .nest("/editor-prefs", editor_prefs::router())
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // Rate-limited public routes (auth callbacks, public share links) +
    // protected routes. `/share/{token}` is unauthenticated and performs a
    // token lookup, so it MUST sit behind the governor like every other
    // public endpoint. Health is intentionally excluded — monitoring tools
    // must never be rate-limited.
    let rate_limited = Router::new()
        .merge(auth::public_router())
        .nest("/share", share::public_share_router())
        .merge(protected)
        .layer(rate_limit);

    // Public routes — no auth required.
    let router = Router::new()
        .route("/health", get(health))
        .merge(rate_limited)
        // Count every response by status class (includes /health). Applied here
        // so it wraps all routes; reads/writes the counters in AppState.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            metrics::track_requests,
        ))
        .layer(cors)
        // 30-second request timeout — returns 408 Request Timeout.
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        // Structured per-request tracing: one span per request carrying method,
        // path, and X-Request-Id; response logged at INFO with latency in ms.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(LatencyUnit::Millis),
                ),
        )
        // Propagate X-Request-Id from requests; generate one if absent.
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state);
    Ok(router)
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let db_status = sqlx::query("SELECT 1")
        .fetch_optional(&state.pool)
        .await
        .map(|r| if r.is_some() { "ok" } else { "error" })
        .unwrap_or("error");

    Json(json!({
        "status": if db_status == "ok" { "ok" } else { "degraded" },
        "service": "ember-trove-api",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": Utc::now().to_rfc3339(),
        "database": db_status
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use tower_governor::governor::GovernorConfigBuilder;

    use super::{E2E_RATE_LIMIT, PROD_RATE_LIMIT, RateLimitBudget, rate_limit_budget};

    /// Verify that every rate-limit budget we can select at startup is valid
    /// (non-zero period and burst size) and therefore `finish()` returns `Some`.
    #[test]
    fn governor_config_is_valid() {
        use tower_governor::key_extractor::SmartIpKeyExtractor;
        for budget in [PROD_RATE_LIMIT, E2E_RATE_LIMIT] {
            let conf = GovernorConfigBuilder::default()
                .key_extractor(SmartIpKeyExtractor)
                .per_millisecond(budget.replenish_ms)
                .burst_size(budget.burst)
                .finish();
            assert!(
                conf.is_some(),
                "governor config must be valid for {budget:?}"
            );
        }
    }

    /// SECURITY: pin the production budget so it can't be loosened by accident.
    #[test]
    fn production_budget_is_ten_per_second_burst_hundred() {
        assert_eq!(
            PROD_RATE_LIMIT,
            RateLimitBudget {
                replenish_ms: 100,
                burst: 100
            }
        );
    }

    /// SECURITY: without the `e2e-bypass` feature (every release image), the
    /// environment can never select the e2e budget — not even E2E_AUTH_BYPASS=1.
    #[test]
    fn release_builds_ignore_the_bypass_env() {
        for env in [None, Some("1"), Some("0"), Some("true")] {
            assert_eq!(
                rate_limit_budget(false, env),
                PROD_RATE_LIMIT,
                "env={env:?}"
            );
        }
    }

    /// e2e builds stay on the production budget unless explicitly armed with
    /// exactly "1" — the same rule as the auth bypass.
    #[test]
    fn e2e_builds_need_the_bypass_armed() {
        for env in [None, Some("0"), Some("true"), Some("")] {
            assert_eq!(rate_limit_budget(true, env), PROD_RATE_LIMIT, "env={env:?}");
        }
        assert_eq!(rate_limit_budget(true, Some("1")), E2E_RATE_LIMIT);
    }

    /// The e2e budget must actually be looser, or the flake it fixes returns.
    /// Checked at compile time: a constant comparison can't fail at runtime,
    /// so a runtime `assert!` here was only ever a lint
    /// (`clippy::assertions_on_constants`).
    const _: () = {
        assert!(E2E_RATE_LIMIT.burst > PROD_RATE_LIMIT.burst);
        assert!(E2E_RATE_LIMIT.replenish_ms < PROD_RATE_LIMIT.replenish_ms);
    };
}
