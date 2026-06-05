use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::header,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::{
    PrivateCookieJar,
    cookie::{Cookie, SameSite},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use common::auth::{AuthClaims, ChangePasswordRequest, UserInfo};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    auth::middleware::SESSION_COOKIE, error::ApiError, repo::pkce::PKCE_TTL, state::AppState,
};

/// Cookie that holds the encrypted refresh token.
/// Scoped to `/api/auth/refresh` so the browser never sends it on other requests.
const REFRESH_COOKIE: &str = "ember_trove_refresh";

/// Cookie that holds the Cognito access token — required by the ChangePassword API.
/// Scoped to `/api/auth/change-password` so it is never sent on other requests.
const ACCESS_COOKIE: &str = "ember_trove_access";

/// Short-lived, encrypted cookie binding the OAuth `state` to the browser that
/// began the login. The callback requires the echoed `state` to match it
/// (CSRF / session-fixation defense). `SameSite=Lax` so it is still sent on the
/// top-level redirect back from the IdP. Scoped to `/api/auth`.
const OAUTH_STATE_COOKIE: &str = "ember_trove_oauth_state";

/// Public auth routes (no JWT required).
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}

/// Protected auth routes (JWT required — layered behind require_auth).
pub fn protected_router() -> Router<AppState> {
    Router::new()
        .route("/auth/me", get(me))
        .route("/auth/change-password", post(change_password))
}

#[derive(Serialize)]
struct RedirectResponse {
    redirect_url: String,
}

async fn login(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> Result<
    (
        PrivateCookieJar,
        [(header::HeaderName, &'static str); 1],
        Json<RedirectResponse>,
    ),
    ApiError,
> {
    let oidc = state
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::Internal("OIDC not configured — auth is disabled".to_string()))?;

    // PKCE: generate a 32-byte random code_verifier and derive the S256 challenge.
    let mut raw = [0u8; 32];
    rand::rng().fill_bytes(&mut raw);
    let code_verifier = URL_SAFE_NO_PAD.encode(raw);
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));

    // Generate a random OAuth state token (16 bytes).
    // The verifier is stored server-side keyed by this token, which travels
    // through the redirect URL — avoiding iOS Safari ITP cookie restrictions.
    let mut state_raw = [0u8; 16];
    rand::rng().fill_bytes(&mut state_raw);
    let oauth_state = URL_SAFE_NO_PAD.encode(state_raw);

    // Persist the verifier keyed by the OAuth state. The background sweeper
    // purges expired entries; the callback consumes this one exactly once.
    state.pkce.store(&oauth_state, &code_verifier).await?;

    // Bind the state to THIS browser (CSRF defense): the callback requires the
    // echoed `state` to equal this encrypted cookie. SameSite=Lax so it rides
    // the top-level redirect back from the IdP; session-lived (cleared at the
    // callback; the server-side PKCE entry also has a 10-min TTL).
    let state_cookie = Cookie::build((OAUTH_STATE_COOKIE, oauth_state.clone()))
        .path("/api/auth")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(state.auth.cookie_secure)
        .build();
    let jar = jar.add(state_cookie);

    let redirect_uri = format!("{}/api/auth/callback", state.auth.api_external_url);
    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=openid+email+profile\
         &code_challenge={}&code_challenge_method=S256&state={}",
        oidc.authorization_endpoint,
        state.auth.client_id,
        urlencoding::encode(&redirect_uri),
        code_challenge,
        oauth_state,
    );

    Ok((
        jar,
        [(header::CACHE_CONTROL, "no-store")],
        Json(RedirectResponse { redirect_url: url }),
    ))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    /// OAuth state parameter — used to retrieve the PKCE verifier.
    state: Option<String>,
}

async fn callback(
    State(app_state): State<AppState>,
    jar: PrivateCookieJar,
    Query(params): Query<CallbackQuery>,
) -> Response {
    // The callback is a browser-navigation endpoint (Cognito redirects the
    // browser here), so on failure we MUST emit a 303 redirect — never the
    // shared JSON `ApiError` response, which the browser would render
    // verbatim as the page. Any failure (missing PKCE verifier, Cognito
    // rejection, OIDC misconfig) bounces the user back to the frontend
    // root, where AuthGate will start a fresh login flow.
    match try_callback(&app_state, jar, params).await {
        Ok((updated_jar, headers, html)) => (updated_jar, headers, html).into_response(),
        Err(e) => {
            tracing::warn!(
                ?e,
                "OAuth callback failed — redirecting to frontend to restart login"
            );
            Redirect::to(&app_state.auth.frontend_url).into_response()
        }
    }
}

/// Inner callback worker — returns `ApiError` on any failure so the outer
/// handler can convert it into a redirect.
async fn try_callback(
    app_state: &AppState,
    jar: PrivateCookieJar,
    params: CallbackQuery,
) -> Result<
    (
        PrivateCookieJar,
        [(header::HeaderName, &'static str); 1],
        Html<String>,
    ),
    ApiError,
> {
    let oidc = app_state
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::Internal("OIDC not configured — auth is disabled".to_string()))?;

    // Retrieve and consume the PKCE verifier from the server-side store.
    // A missing verifier means the entry was evicted (10-min TTL elapsed
    // at the Cognito login screen, or the api container restarted between
    // /login and /callback). Treat as Unauthorized so the outer handler
    // redirects to a fresh login flow.
    let oauth_state = params
        .state
        .as_deref()
        .ok_or_else(|| ApiError::Unauthorized("missing oauth state".to_string()))?;

    // SECURITY (CSRF / session fixation): the `state` echoed back by the IdP
    // must match the value bound to THIS browser at /login via an encrypted
    // cookie. Without this, an attacker could feed a victim a valid
    // (state, code) pair and log them into the attacker's account.
    let state_ok = jar
        .get(OAUTH_STATE_COOKIE)
        .is_some_and(|c| c.value() == oauth_state);
    if !state_ok {
        return Err(ApiError::Unauthorized("oauth state mismatch".to_string()));
    }

    let code_verifier = app_state
        .pkce
        .take(oauth_state, PKCE_TTL)
        .await?
        .ok_or_else(|| {
            ApiError::Unauthorized(
                "PKCE verifier not found (login expired or server restarted)".to_string(),
            )
        })?;

    let redirect_uri = format!("{}/api/auth/callback", app_state.auth.api_external_url);
    let token_resp = oidc
        .exchange_code(&params.code, &redirect_uri, Some(&code_verifier))
        .await?;

    // Prefer the ID token for the session cookie — it carries email, name, and
    // cognito:groups which the auth middleware needs.  Fall back to access_token
    // for providers that don't issue a separate ID token.
    let cognito_access_token = token_resp.access_token;
    let session_token = token_resp
        .id_token
        .unwrap_or_else(|| cognito_access_token.clone());

    let secure = app_state.auth.cookie_secure;

    let session_cookie = Cookie::build((SESSION_COOKIE, session_token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .build();

    // Store the Cognito access token in a tightly-scoped cookie so the
    // change-password endpoint can call the Cognito ChangePassword API.
    let cognito_access_cookie = Cookie::build((ACCESS_COOKIE, cognito_access_token))
        .path("/api/auth/change-password")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .build();

    // Clear the one-time OAuth state cookie now that it has served its purpose.
    let mut updated_jar = jar
        .remove(
            Cookie::build((OAUTH_STATE_COOKIE, ""))
                .path("/api/auth")
                .build(),
        )
        .add(session_cookie)
        .add(cognito_access_cookie);

    if let Some(refresh_token) = token_resp.refresh_token {
        let refresh_cookie = Cookie::build((REFRESH_COOKIE, refresh_token))
            .path("/api/auth/refresh")
            .http_only(true)
            .same_site(SameSite::Strict)
            .secure(secure)
            .build();
        updated_jar = updated_jar.add(refresh_cookie);
    }

    let frontend_url = &app_state.auth.frontend_url;

    let html = format!(
        r#"<!DOCTYPE html>
<html><head><title>Redirecting...</title></head>
<body><script>window.location.replace("{frontend_url}");</script></body>
</html>"#
    );

    Ok((
        updated_jar,
        [(header::CACHE_CONTROL, "no-store")],
        Html(html),
    ))
}

async fn refresh(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> Result<(PrivateCookieJar, Json<serde_json::Value>), ApiError> {
    let oidc = state
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::Internal("OIDC not configured".to_string()))?;

    let refresh_token = jar
        .get(REFRESH_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| ApiError::Unauthorized("no refresh token".to_string()))?;

    let token_resp = oidc.exchange_refresh_token(&refresh_token).await?;

    let session_token = token_resp.id_token.unwrap_or(token_resp.access_token);
    let secure = state.auth.cookie_secure;

    let access_cookie = Cookie::build((SESSION_COOKIE, session_token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure)
        .build();

    let mut updated_jar = jar.add(access_cookie);

    // Cognito rotates the refresh token — update the cookie with the new one.
    if let Some(new_refresh) = token_resp.refresh_token {
        let refresh_cookie = Cookie::build((REFRESH_COOKIE, new_refresh))
            .path("/api/auth/refresh")
            .http_only(true)
            .same_site(SameSite::Strict)
            .secure(secure)
            .build();
        updated_jar = updated_jar.add(refresh_cookie);
    }

    Ok((updated_jar, Json(serde_json::json!({"ok": true}))))
}

async fn me(Extension(claims): Extension<AuthClaims>) -> Json<UserInfo> {
    Json(UserInfo::from(claims))
}

async fn logout(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> (PrivateCookieJar, Json<RedirectResponse>) {
    // Revoke the refresh token server-side (RFC 7009).
    if let Some(oidc) = state.oidc.as_ref()
        && let Some(refresh_token) = jar.get(REFRESH_COOKIE).map(|c| c.value().to_string())
    {
        oidc.backchannel_logout(&refresh_token).await;
    }

    let updated_jar = jar
        .remove(Cookie::build((SESSION_COOKIE, "")).path("/").build())
        .remove(
            Cookie::build((REFRESH_COOKIE, ""))
                .path("/api/auth/refresh")
                .build(),
        )
        .remove(
            Cookie::build((ACCESS_COOKIE, ""))
                .path("/api/auth/change-password")
                .build(),
        );

    // Redirect through Cognito's end-session endpoint so the SSO session cookie
    // at the Cognito domain is cleared.  Without this the browser is silently
    // re-authenticated on the very next /api/auth/login round-trip.
    let redirect_url = if let Some(oidc) = state.oidc.as_ref() {
        format!(
            "{}?client_id={}&logout_uri={}",
            oidc.end_session_endpoint,
            state.auth.client_id,
            urlencoding::encode(&state.auth.frontend_url),
        )
    } else {
        state.auth.frontend_url.clone()
    };

    (updated_jar, Json(RedirectResponse { redirect_url }))
}

async fn change_password(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Retrieve the Cognito access token stored during login.
    let access_token = jar
        .get(ACCESS_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| {
            ApiError::Unauthorized(
                "access token cookie missing — please log out and back in".to_string(),
            )
        })?;

    // Call Cognito ChangePassword — requires the access token (not ID token).
    let oidc = state
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::Internal("OIDC not configured".to_string()))?;

    oidc.change_password(&access_token, &body.current_password, &body.new_password)
        .await?;

    Ok(Json(serde_json::json!({"ok": true})))
}
