use common::auth::UserInfo;
use gloo_net::http::Request;

use super::{RedirectResponse, api_url, get_json, parse_json};
use crate::error::UiError;

pub async fn fetch_me() -> Result<UserInfo, UiError> {
    let resp = Request::get(&api_url("/auth/me"))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    // Do NOT use parse_json here — parse_json retries via refresh+reload on
    // 401, which would loop infinitely because fetch_me IS the auth check.
    // A plain 401 here just means "not logged in"; AuthGate handles the redirect.
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

/// Call the backend refresh endpoint. On success the server sets a new
/// session cookie. The UI then reloads to pick it up.
pub async fn refresh_session() -> Result<(), UiError> {
    let resp = Request::post(&api_url("/auth/refresh"))
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
