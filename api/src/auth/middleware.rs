use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use axum_extra::extract::PrivateCookieJar;
use common::auth::AuthClaims;

use crate::{error::ApiError, state::AppState};

use super::oidc::OidcClient;

pub const SESSION_COOKIE: &str = "ember_trove_session";

/// Middleware that extracts and validates a JWT from the session cookie or
/// Authorization header, injecting `AuthClaims` into request extensions.
pub async fn require_auth(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // ── E2E auth bypass ──────────────────────────────────────────────────
    // Compiled ONLY with the `e2e-bypass` cargo feature, which no release
    // artifact enables (deploy/Dockerfile.api builds without features), and
    // armed ONLY when E2E_AUTH_BYPASS=1 is set at runtime. Lets Playwright
    // drive the UI without a Cognito session — see e2e/README.md.
    #[cfg(feature = "e2e-bypass")]
    if std::env::var("E2E_AUTH_BYPASS").as_deref() == Ok("1") {
        let claims = AuthClaims {
            sub: std::env::var("E2E_BYPASS_SUB").unwrap_or_else(|_| "e2e-test-user".to_string()),
            email: Some("e2e@test.local".to_string()),
            name: Some("E2E Test".to_string()),
            roles: vec![],
            exp: i64::MAX,
        };
        request.extensions_mut().insert(claims);
        return Ok(next.run(request).await);
    }

    let oidc = state
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::Internal("OIDC not configured — auth is disabled".to_string()))?;

    let token = extract_token(&jar, &request)?;
    let claims = validate_and_map(oidc, &token).await?;

    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

/// Try to get the JWT from the session cookie first, then the Authorization header.
fn extract_token(jar: &PrivateCookieJar, request: &Request) -> Result<String, ApiError> {
    // 1. Session cookie
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let value = cookie.value().to_string();
        if !value.is_empty() {
            return Ok(value);
        }
    }

    // 2. Authorization: Bearer <token>
    if let Some(auth_header) = request.headers().get("authorization") {
        let header_str = auth_header
            .to_str()
            .map_err(|_| ApiError::Unauthorized("invalid authorization header".to_string()))?;
        if let Some(token) = header_str.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Ok(token.to_string());
            }
        }
    }

    Err(ApiError::Unauthorized("missing authentication".to_string()))
}

/// Validate the JWT (ID token) and map OIDC claims to our `AuthClaims`.
///
/// `cognito:groups` becomes the roles list; absent means no groups (empty vec).
async fn validate_and_map(oidc: &OidcClient, token: &str) -> Result<AuthClaims, ApiError> {
    let claims = oidc.validate_token(token).await?;

    let roles = claims.groups.unwrap_or_default();

    Ok(AuthClaims {
        sub: claims.sub,
        email: claims.email,
        name: claims.name,
        roles,
        exp: claims.exp,
    })
}
