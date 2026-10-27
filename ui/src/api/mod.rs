#![allow(dead_code)]

mod activity;
mod admin;
mod attachments;
mod auth;
mod backup;
mod edges;
mod editor_prefs;
mod favorites;
mod graph;
mod inbox;
mod node_links;
mod nodes;
mod notes;
mod people;
mod permissions;
mod search;
mod tags;
mod tasks;
mod templates;
mod versions;
mod webhooks;

use common::session_retry::{
    Next, REFRESH_TIMEOUT_MS, RefreshGate, RefreshOutcome, after_refresh, after_retry,
    refresh_outcome,
};
use gloo_net::http::{Request, Response};
use gloo_timers::callback::Timeout;
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
        let msg = resp
            .text()
            .await
            .unwrap_or_else(|_| "password change failed".to_string());
        Err(UiError::api(status, msg))
    }
}

// ── Session renewal ─────────────────────────────────────────────────────

thread_local! {
    /// One session refresh at a time for the whole app (`common::session_retry`).
    static SESSION: RefreshGate = RefreshGate::default();
}

/// Send the request `build` makes. On a 401, renew the session (one refresh
/// shared by every call that got a 401 meanwhile) and send it again, once,
/// in place: no page reload (#161 follow-up). A refused refresh, or a 401
/// on the retry, goes to the login page and never returns. A refresh that
/// got no answer (offline, 5xx) is an ordinary error the caller can retry.
///
/// `build` runs again for the retry: a request (and its body) is consumed
/// by sending it. The first attempt changed nothing, since the API rejects
/// an unauthenticated call before any handler runs.
pub(crate) async fn send_authed(
    build: impl Fn() -> Result<Request, UiError>,
) -> Result<Response, UiError> {
    let gate = SESSION.with(RefreshGate::clone);
    let sent = gate.generation();
    let resp = send(build()?).await?;
    if resp.status() != 401 {
        return Ok(resp);
    }
    match after_refresh(gate.refresh(sent, renew_session).await) {
        Next::Retry => {
            let resp = send(build()?).await?;
            match after_retry(resp.status()) {
                Some(_) => to_login().await,
                None => Ok(resp),
            }
        }
        Next::Login => to_login().await,
        Next::Fail => Err(UiError::Network(
            "couldn't renew your session (offline?); try again".to_string(),
        )),
    }
}

async fn send(req: Request) -> Result<Response, UiError> {
    req.send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))
}

/// `POST /auth/refresh`, classified (only 401/403 ends the session), and
/// abandoned after `REFRESH_TIMEOUT_MS`: every other 401 waits on it.
async fn renew_session() -> RefreshOutcome {
    let abort = web_sys::AbortController::new().ok();
    let signal = abort.as_ref().map(web_sys::AbortController::signal);
    // Dropping the Timeout at the end cancels it.
    let _deadline = abort.map(|a| Timeout::new(REFRESH_TIMEOUT_MS, move || a.abort()));
    match auth::refresh_session(signal.as_ref()).await {
        Ok(()) => RefreshOutcome::Refreshed,
        Err(UiError::Api { status, .. }) => refresh_outcome(Some(status)),
        Err(_) => refresh_outcome(None),
    }
}

/// Go to the login page. Parks until the navigation tears down the WASM
/// runtime, so the caller never sees an error for a call it can't make.
/// `spawn_local` avoids a recursive async fn (fetch_login_url calls
/// `parse_json` internally).
async fn to_login<T>() -> T {
    wasm_bindgen_futures::spawn_local(async {
        if let Ok(url) = auth::fetch_login_url().await
            && let Some(win) = web_sys::window()
        {
            let _ = win.location().set_href(&url);
        }
    });
    std::future::pending::<T>().await
}

// ── Shared JSON parser ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RedirectResponse {
    pub redirect_url: String,
}

/// Convert a non-2xx response into a `UiError`. A 401 that reaches here
/// already had its refresh and retry in [`send_authed`] (or comes from a
/// call that doesn't renew, like logout), so it goes to the login page.
async fn handle_error_response(response: Response) -> UiError {
    let status = response.status();
    if status == 401 {
        return to_login().await;
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
/// [`handle_error_response`].
pub async fn parse_json<T: serde::de::DeserializeOwned>(response: Response) -> Result<T, UiError> {
    if response.ok() {
        response
            .json::<T>()
            .await
            .map_err(|e| UiError::Parse(e.to_string()))
    } else {
        Err(handle_error_response(response).await)
    }
}

/// Like [`parse_json`] but for endpoints that return no body.
pub async fn parse_empty(response: Response) -> Result<(), UiError> {
    if response.ok() {
        Ok(())
    } else {
        Err(handle_error_response(response).await)
    }
}

// ── HTTP verb helpers ─────────────────────────────────────────────────────
// Fold the repeated `Request::X(&api_url(path))…send…parse` boilerplate.
// Every one goes through [`send_authed`], so a 401 renews and retries.

fn built(r: Result<Request, gloo_net::Error>) -> Result<Request, UiError> {
    r.map_err(|e| UiError::Parse(e.to_string()))
}

/// A GET of the full `url`, for [`send_authed`] callers that build a query.
pub(crate) fn get_request(url: &str) -> Result<Request, UiError> {
    built(Request::get(url).build())
}

/// GET `path`, parse a JSON body into `T`.
pub async fn get_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, UiError> {
    let url = api_url(path);
    parse_json(send_authed(|| built(Request::get(&url).build())).await?).await
}

/// POST `body` as JSON to `path`, parse a JSON response into `T`.
pub async fn post_json<T, B>(path: &str, body: &B) -> Result<T, UiError>
where
    T: serde::de::DeserializeOwned,
    B: Serialize,
{
    let url = api_url(path);
    parse_json(send_authed(|| built(Request::post(&url).json(body))).await?).await
}

/// PATCH `body` as JSON to `path`, parse a JSON response into `T`.
pub async fn patch_json<T, B>(path: &str, body: &B) -> Result<T, UiError>
where
    T: serde::de::DeserializeOwned,
    B: Serialize,
{
    let url = api_url(path);
    parse_json(send_authed(|| built(Request::patch(&url).json(body))).await?).await
}

/// PUT `body` as JSON to `path`, parse a JSON response into `T`.
pub async fn put_json<T, B>(path: &str, body: &B) -> Result<T, UiError>
where
    T: serde::de::DeserializeOwned,
    B: Serialize,
{
    let url = api_url(path);
    parse_json(send_authed(|| built(Request::put(&url).json(body))).await?).await
}

/// DELETE `path`, expecting no response body.
pub async fn delete_empty(path: &str) -> Result<(), UiError> {
    let url = api_url(path);
    parse_empty(send_authed(|| built(Request::delete(&url).build())).await?).await
}

/// PUT `body` as JSON to `path`, expecting no response body.
pub async fn put_empty<B: Serialize>(path: &str, body: &B) -> Result<(), UiError> {
    let url = api_url(path);
    parse_empty(send_authed(|| built(Request::put(&url).json(body))).await?).await
}

/// POST `body` as JSON to `path`, expecting no response body.
pub async fn post_empty<B: Serialize>(path: &str, body: &B) -> Result<(), UiError> {
    let url = api_url(path);
    parse_empty(send_authed(|| built(Request::post(&url).json(body))).await?).await
}

/// POST `path` with no request or response body — a pure action trigger
/// (restore, attach).
pub async fn post_action(path: &str) -> Result<(), UiError> {
    let url = api_url(path);
    parse_empty(send_authed(|| built(Request::post(&url).build())).await?).await
}

/// POST `path` with no request body, parse a JSON response into `T`.
pub async fn post_action_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, UiError> {
    let url = api_url(path);
    parse_json(send_authed(|| built(Request::post(&url).build())).await?).await
}

/// PUT `path` with no request body, parse a JSON response into `T`.
pub async fn put_action_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, UiError> {
    let url = api_url(path);
    parse_json(send_authed(|| built(Request::put(&url).build())).await?).await
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
pub use editor_prefs::*;
pub use favorites::*;
pub use graph::*;
pub use inbox::*;
pub use node_links::*;
pub use nodes::*;
pub use notes::*;
pub use people::*;
pub use permissions::*;
pub use search::*;
pub use tags::*;
pub use tasks::*;
pub use templates::*;
pub use versions::*;
pub use webhooks::*;
