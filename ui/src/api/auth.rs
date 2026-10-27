use common::auth::UserInfo;
use gloo_net::http::Request;

use super::{RedirectResponse, api_url, get_json, parse_json};
use crate::error::UiError;

pub async fn fetch_me() -> Result<UserInfo, UiError> {
    let resp = Request::get(&api_url("/auth/me"))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    // Do NOT use send_authed/parse_json here: they renew the session (or go
    // to the login page) on a 401, and fetch_me IS the auth check. A plain
    // 401 here just means "not logged in"; AuthGate handles the redirect.
    if resp.ok() {
        resp.json::<UserInfo>()
            .await
            .map_err(|e| UiError::Parse(e.to_string()))
    } else {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        Err(UiError::api(status, text))
    }
}

pub async fn fetch_login_url() -> Result<String, UiError> {
    let data: RedirectResponse = get_json("/auth/login").await?;
    Ok(data.redirect_url)
}

/// Call the backend refresh endpoint. On success the server sets new
/// session cookies, which the browser sends with the retried call
/// (`api::send_authed`). `signal` lets the caller abandon a hung refresh.
pub async fn refresh_session(signal: Option<&web_sys::AbortSignal>) -> Result<(), UiError> {
    let resp = Request::post(&api_url("/auth/refresh"))
        .abort_signal(signal)
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    if resp.ok() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        Err(UiError::api(status, text))
    }
}

pub async fn fetch_logout_url() -> Result<String, UiError> {
    let resp = Request::post(&api_url("/auth/logout"))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    let data: RedirectResponse = parse_json(resp).await?;
    Ok(data.redirect_url)
}
