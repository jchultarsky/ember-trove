#![allow(dead_code)]

mod activity;
mod admin;
mod attachments;
mod auth;
mod backup;
mod edges;
mod favorites;
mod graph;
mod inbox;
mod node_links;
mod nodes;
mod notes;
mod permissions;
mod search;
mod share;
mod tags;
mod tasks;
mod templates;
mod versions;

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};

use crate::error::UiError;

const API_BASE: &str = "/api";

#[must_use]
pub fn api_url(path: &str) -> String {
    format!("{API_BASE}{path}")
}

// ── Health ─────────────────────────────────���──────────────────────────────

#[derive(Deserialize)]
struct HealthResponse {
    version: String,
}

/// Fetch the API version string from `/api/health`.
pub async fn fetch_api_version() -> String {
    let Ok(resp) = Request::get(&api_url("/health")).send().await else {
        return String::new();
    };
    resp.json::<HealthResponse>()
        .await
        .map(|h| h.version)
        .unwrap_or_default()
}

/// Change the current user's password.
pub async fn change_password(current: &str, proposed: &str) -> Result<(), UiError> {
    let resp = Request::post(&api_url("/auth/change-password"))
        .json(&serde_json::json!({
            "current_password": current,
            "new_password": proposed,
        }))
        .map_err(|e| UiError::Network(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;

    if resp.ok() {
        Ok(())
    } else {
        let status = resp.status();
        let msg = resp.text().await.unwrap_or_else(|_| "password change failed".to_string());
        Err(UiError::api(status, msg))
    }
}

// ── Shared JSON parser ──────────────────────────────────���────────────────

#[derive(Deserialize)]
pub struct RedirectResponse {
    pub redirect_url: String,
}

/// Convert a non-2xx response into a `UiError`, handling 401 centrally: try a
/// silent token refresh + full-page reload, else redirect to login. Both 401
/// branches park forever (the page navigation tears down the WASM runtime), so
/// this never returns in the 401 case. Shared by `parse_json` and `parse_empty`
/// so EVERY API call — body or no-body — gets the same auto-refresh behavior.
async fn handle_error_response(response: gloo_net::http::Response) -> UiError {
    let status = response.status();
    if status == 401 {
        // Try a silent token refresh first. If it succeeds, a full-page reload
        // picks up the new access token and retries pending calls invisibly.
        if auth::refresh_session().await.is_ok() {
            if let Some(win) = web_sys::window() {
                let _ = win.location().reload();
            }
            // Park until the reload destroys the WASM runtime. Without this the
            // Err propagates to callers (e.g. on_save in NodeEditor) before the
            // reload fires, causing the save to silently fail with no feedback.
            std::future::pending::<()>().await;
        } else {
            // Refresh token also expired (long idle, server restart, etc.).
            // Redirect to login rather than leaving a blank screen or a
            // confusing "server error 401". spawn_local avoids a recursive
            // async fn (fetch_login_url calls parse_json internally).
            wasm_bindgen_futures::spawn_local(async {
                if let Ok(url) = auth::fetch_login_url().await
                    && let Some(win) = web_sys::window()
                {
                    let _ = win.location().set_href(&url);
                }
            });
            std::future::pending::<()>().await;
        }
    }
    let text = response
        .text()
        .await
        .unwrap_or_else(|_| "unknown error".to_string());
    // Extract the `error` field if the body is `{"error": "..."}`, so the UI
    // shows a human-readable message rather than a raw JSON string.
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .unwrap_or(text);
    UiError::api(status, message)
}

/// Parse a JSON response body, or convert a non-2xx response via
/// [`handle_error_response`] (including the 401 silent-refresh path).
pub async fn parse_json<T: serde::de::DeserializeOwned>(
    response: gloo_net::http::Response,
) -> Result<T, UiError> {
    if response.ok() {
        response
            .json::<T>()
            .await
            .map_err(|e| UiError::Parse(e.to_string()))
    } else {
        Err(handle_error_response(response).await)
    }
}

/// Like [`parse_json`] but for endpoints that return no body. Routes 401 through
/// the SAME silent-refresh path as `parse_json` — previously the hand-rolled
/// `if resp.ok()` blocks in delete/reorder calls bypassed it, so an expired
/// session errored instead of refreshing.
pub async fn parse_empty(response: gloo_net::http::Response) -> Result<(), UiError> {
    if response.ok() {
        Ok(())
    } else {
        Err(handle_error_response(response).await)
    }
}

// ── HTTP verb helpers ─────────────────────────────────────────────────────
// Fold the repeated `Request::X(&api_url(path))…send().map_err(Network)…parse`
// boilerplate. All route 401 through the shared refresh path above.

/// GET `path`, parse a JSON body into `T`.
pub async fn get_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, UiError> {
    let resp = Request::get(&api_url(path))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// POST `body` as JSON to `path`, parse a JSON response into `T`.
pub async fn post_json<T, B>(path: &str, body: &B) -> Result<T, UiError>
where
    T: serde::de::DeserializeOwned,
    B: Serialize,
{
    let resp = Request::post(&api_url(path))
        .json(body)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// PATCH `body` as JSON to `path`, parse a JSON response into `T`.
pub async fn patch_json<T, B>(path: &str, body: &B) -> Result<T, UiError>
where
    T: serde::de::DeserializeOwned,
    B: Serialize,
{
    let resp = Request::patch(&api_url(path))
        .json(body)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// PUT `body` as JSON to `path`, parse a JSON response into `T`.
pub async fn put_json<T, B>(path: &str, body: &B) -> Result<T, UiError>
where
    T: serde::de::DeserializeOwned,
    B: Serialize,
{
    let resp = Request::put(&api_url(path))
        .json(body)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// DELETE `path`, expecting no response body.
pub async fn delete_empty(path: &str) -> Result<(), UiError> {
    let resp = Request::delete(&api_url(path))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_empty(resp).await
}

/// PUT `body` as JSON to `path`, expecting no response body.
pub async fn put_empty<B: Serialize>(path: &str, body: &B) -> Result<(), UiError> {
    let resp = Request::put(&api_url(path))
        .json(body)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_empty(resp).await
}

/// POST `body` as JSON to `path`, expecting no response body.
pub async fn post_empty<B: Serialize>(path: &str, body: &B) -> Result<(), UiError> {
    let resp = Request::post(&api_url(path))
        .json(body)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_empty(resp).await
}

/// POST `path` with no request or response body — a pure action trigger
/// (restore, attach). Routes 401 through the shared refresh path.
pub async fn post_action(path: &str) -> Result<(), UiError> {
    let resp = Request::post(&api_url(path))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_empty(resp).await
}

/// POST `path` with no request body, parse a JSON response into `T`.
pub async fn post_action_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, UiError> {
    let resp = Request::post(&api_url(path))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// PUT `path` with no request body, parse a JSON response into `T`.
pub async fn put_action_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, UiError> {
    let resp = Request::put(&api_url(path))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

// ── Re-exports ───────────────────────────────────────────────────────────
// All public items re-exported at `crate::api::*` so existing call-sites
// (`crate::api::fetch_node(...)`) continue to compile unchanged.

pub use activity::*;
pub use admin::*;
pub use attachments::*;
pub use auth::*;
pub use backup::*;
pub use edges::*;
pub use favorites::*;
pub use graph::*;
pub use inbox::*;
pub use node_links::*;
pub use nodes::*;
pub use notes::*;
pub use permissions::*;
pub use search::*;
pub use share::*;
pub use tags::*;
pub use tasks::*;
pub use templates::*;
pub use versions::*;
