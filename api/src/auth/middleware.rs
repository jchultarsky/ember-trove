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
        let header = request
            .headers()
            .get(E2E_SUBJECT_HEADER)
            .map(|v| v.to_str().unwrap_or("\u{fffd}"));
        let default_sub =
            std::env::var("E2E_BYPASS_SUB").unwrap_or_else(|_| E2E_DEFAULT_SUB.to_string());
        let claims = e2e_bypass_claims(header, &default_sub)?;
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

/// Header that picks the e2e test identity (only while the bypass is armed).
#[cfg(feature = "e2e-bypass")]
const E2E_SUBJECT_HEADER: &str = "x-e2e-subject";
#[cfg(feature = "e2e-bypass")]
const E2E_DEFAULT_SUB: &str = "e2e-test-user";
/// The second test identity (#135), for sharing tests: user A shares a node
/// with user B, same-titled nodes across owners, permission boundaries.
#[cfg(any(test, feature = "e2e-bypass"))]
pub const E2E_OTHER_SUB: &str = "e2e-other-user";

/// The bypass's claims for an optional `x-e2e-subject` header.
///
/// SECURITY: compiled only for tests and `e2e-bypass` builds (never a
/// release image), and reached only while `E2E_AUTH_BYPASS=1`. The header
/// selects one of two FIXED identities. It is never used as a subject id,
/// and any other value is rejected, so a header can't impersonate an
/// arbitrary user even in a test stack.
#[cfg(any(test, feature = "e2e-bypass"))]
fn e2e_bypass_claims(header: Option<&str>, default_sub: &str) -> Result<AuthClaims, ApiError> {
    let (sub, email, name) = match header {
        None => (default_sub, "e2e@test.local", "E2E Test"),
        Some("other") => (E2E_OTHER_SUB, "e2e-other@test.local", "E2E Other"),
        Some(_) => {
            return Err(ApiError::Unauthorized(
                "unknown e2e test subject".to_string(),
            ));
        }
    };
    Ok(AuthClaims {
        sub: sub.to_string(),
        email: Some(email.to_string()),
        name: Some(name.to_string()),
        roles: vec![],
        exp: i64::MAX,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e2e_bypass_defaults_to_the_configured_subject() {
        let c = e2e_bypass_claims(None, "e2e-test-user").expect("claims");
        assert_eq!(c.sub, "e2e-test-user");
        assert!(c.roles.is_empty(), "the test identity is never an admin");
    }

    #[test]
    fn e2e_bypass_other_selects_the_fixed_second_identity() {
        let c = e2e_bypass_claims(Some("other"), "e2e-test-user").expect("claims");
        assert_eq!(c.sub, E2E_OTHER_SUB);
        assert!(c.roles.is_empty());
    }

    /// SECURITY: the header is a selector, never a subject id.
    #[test]
    fn e2e_bypass_rejects_any_other_subject_value() {
        for v in [
            "",
            "admin",
            "f1eb2590-0091-70e4-d9b3-24e4a23d24d1",
            "e2e-other-user",
            "Other",
        ] {
            assert!(
                matches!(
                    e2e_bypass_claims(Some(v), "e2e-test-user"),
                    Err(ApiError::Unauthorized(_))
                ),
                "{v:?} must be rejected"
            );
        }
    }
}
