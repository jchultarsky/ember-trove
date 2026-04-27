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
use serde::Deserialize;

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

pub async fn parse_json<T: serde::de::DeserializeOwned>(
    response: gloo_net::http::Response,
) -> Result<T, UiError> {
    if response.ok() {
        response
            .json::<T>()
            .await
            .map_err(|e| UiError::Parse(e.to_string()))
    } else {
        let status = response.status();
        if status == 401 {
            // Try a silent token refresh first. If it succeeds, a full-page
            // reload picks up the new access token and retries all pending
            // API calls without the user noticing.
            if auth::refresh_session().await.is_ok() {
                if let Some(win) = web_sys::window() {
                    let _ = win.location().reload();
                }
                // Park this future until the page reload destroys the WASM
                // runtime. Without this, the Err below propagates to callers
                // (e.g. on_save in NodeEditor) before the reload fires, causing
                // the save to silently fail with no user-visible error.
                std::future::pending::<()>().await;
            } else {
                // Refresh token also expired (long idle, server restart, etc.).
                // Redirect to login rather than leaving the user with a blank
                // screen or a confusing "server error 401" message.
                // spawn_local avoids a recursive async fn (fetch_login_url calls
                // parse_json internally).
                wasm_bindgen_futures::spawn_local(async {
                    if let Ok(url) = auth::fetch_login_url().await
                        && let Some(win) = web_sys::window()
                    {
                        let _ = win.location().set_href(&url);
                    }
                });
                // Park until the navigation destroys the WASM runtime so the
                // Err does not reach any caller that might clear the UI state.
                std::future::pending::<()>().await;
            }
        }
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        // Extract the `error` field if the body is a JSON object like
        // `{"error": "..."}`, so the message shown in the UI is human-readable
        // rather than a raw JSON string.
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
            .unwrap_or(text);
        Err(UiError::api(status, message))
    }
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
