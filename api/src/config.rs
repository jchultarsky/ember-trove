use std::env;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    MissingVar(&'static str),

    #[error("invalid value for {var}: {reason}")]
    InvalidValue { var: &'static str, reason: String },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    // S3 — optional until Phase 6
    pub s3_endpoint: Option<String>,
    pub s3_bucket: Option<String>,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    pub s3_region: String,
    // OIDC — optional for Phase 1 dev, required for auth
    pub oidc_issuer: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    // Cognito Admin credentials — optional; enables /admin/* endpoints when set.
    pub cognito_user_pool_id: Option<String>,
    pub cognito_region: String,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    /// Verified SES sender address. When set, invite notifications are emailed to
    /// existing users granted access to a node. Optional — invites still work without it.
    pub ses_from_email: Option<String>,
    // Cookie encryption key (128 hex chars → 64 bytes, required by cookie::Key)
    pub cookie_key: String,
    /// Set `Secure` on session cookies. `true` in production (HTTPS), `false` in dev.
    pub cookie_secure: bool,
    // URLs
    pub frontend_url: String,
    pub api_external_url: String,
    pub host: String,
    pub port: u16,
}

impl Default for Config {
    /// Returns a zero-value Config suitable for unit tests.
    ///
    /// All optional fields are `None`; required string fields use empty or
    /// placeholder values. `test_state()` in tests.rs overrides the fields it
    /// cares about via struct update syntax: `Config { field: val, ..Config::default() }`.
    /// This means adding new optional fields to Config never breaks the test
    /// initializer — the compiler will use the `None` default automatically.
    fn default() -> Self {
        Self {
            database_url: String::new(),
            s3_endpoint: None,
            s3_bucket: None,
            s3_access_key: None,
            s3_secret_key: None,
            s3_region: "us-east-1".to_string(),
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            cognito_user_pool_id: None,
            cognito_region: "us-east-2".to_string(),
            aws_access_key_id: None,
            aws_secret_access_key: None,
            ses_from_email: None,
            cookie_key: "a".repeat(128),
            cookie_secure: false,
            frontend_url: "http://localhost:3000".to_string(),
            api_external_url: "http://localhost:3003".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3003,
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let port = env::var("PORT")
            .unwrap_or_else(|_| "3003".to_string())
            .parse::<u16>()
            .map_err(|e| ConfigError::InvalidValue {
                var: "PORT",
                reason: e.to_string(),
            })?;

        let cookie_key = require("COOKIE_KEY")?;
        if cookie_key.len() != 128 {
            return Err(ConfigError::InvalidValue {
                var: "COOKIE_KEY",
                reason: "must be exactly 128 hex characters (64 bytes)".to_string(),
            });
        }

        Ok(Self {
            database_url: require("DATABASE_URL")?,
            s3_endpoint: optional("S3_ENDPOINT"),
            s3_bucket: optional("S3_BUCKET"),
            s3_access_key: optional("S3_ACCESS_KEY"),
            s3_secret_key: optional("S3_SECRET_KEY"),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            oidc_issuer: optional("OIDC_ISSUER"),
            oidc_client_id: optional("OIDC_CLIENT_ID"),
            oidc_client_secret: optional("OIDC_CLIENT_SECRET"),
            cognito_user_pool_id: optional("COGNITO_USER_POOL_ID"),
            cognito_region: env::var("COGNITO_REGION").unwrap_or_else(|_| "us-east-2".to_string()),
            aws_access_key_id: env::var("AWS_ACCESS_KEY_ID").ok(),
            aws_secret_access_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
            ses_from_email: env::var("SES_FROM_EMAIL").ok(),
            cookie_key,
            // Secure-by-default: a prod deploy that forgets to set COOKIE_SECURE
            // still gets Secure cookies. Local http dev must opt out explicitly
            // (COOKIE_SECURE=false), which the dev docker-compose.yml sets.
            cookie_secure: env::var("COOKIE_SECURE")
                .map(|v| !(v == "false" || v == "0"))
                .unwrap_or(true),
            frontend_url: require("FRONTEND_URL")?,
            api_external_url: require("API_EXTERNAL_URL")?,
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port,
        })
    }
}

fn require(name: &'static str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::MissingVar(name))
}

/// Optional env var; an empty (or whitespace-only) value counts as unset.
/// Lets docker-compose overrides disable a feature by blanking the variable
/// — compose has no way to *unset* an inherited variable.
fn optional(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}
