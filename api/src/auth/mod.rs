pub mod middleware;
pub mod oidc;
pub mod permissions;
pub mod suspended;

/// Configuration for OIDC / JWT validation, loaded at startup.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// OIDC issuer URL (Cognito: `https://cognito-idp.<region>.amazonaws.com/<pool-id>`).
    pub issuer: String,
    /// OIDC client ID.
    pub client_id: String,
    /// OIDC client secret.
    pub client_secret: String,
    /// Frontend origin URL for redirects after login/logout.
    pub frontend_url: String,
    /// API external URL (used to build the callback redirect_uri).
    pub api_external_url: String,
    /// Set the `Secure` flag on session cookies.
    /// Must be `true` in production (HTTPS); `false` in local dev (HTTP).
    pub cookie_secure: bool,
}
