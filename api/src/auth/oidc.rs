use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::ApiError;

/// TTL for JWKS cache — refresh after 1 hour to handle key rotation.
const JWKS_TTL: Duration = Duration::from_secs(3600);

/// True when the token endpoint belongs to AWS Cognito — the only provider
/// whose proprietary `ChangePassword` service call we implement. Parses the
/// URL and matches the HOST suffix (never a substring of the whole URL, so
/// `x.amazonaws.com.evil.example` does not qualify).
fn is_cognito_token_endpoint(token_endpoint: &str) -> bool {
    reqwest::Url::parse(token_endpoint)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h == "amazonaws.com" || h.ends_with(".amazonaws.com"))
        })
        .unwrap_or(false)
}

/// Cached JWKS with timestamp for TTL invalidation.
struct CachedJwks {
    jwks: JwkSet,
    fetched_at: Instant,
}

/// OIDC discovery document (subset of fields we need).
#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    /// The issuer identifier — must equal the `iss` claim of issued tokens.
    #[serde(default)]
    issuer: Option<String>,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    end_session_endpoint: String,
    /// RFC 7009 token revocation endpoint — used for backchannel logout.
    revocation_endpoint: Option<String>,
}

/// Token response from the OIDC token endpoint.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// ID token — present for `openid` scope; contains email, name, and group claims.
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

/// JWT claims — compatible with Cognito ID tokens.
///
/// Groups are read from `cognito:groups` (Cognito) which maps directly
/// to roles in our `AuthClaims`.
#[derive(Debug, Deserialize)]
pub struct OidcClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    /// Cognito group memberships — used as roles in `AuthClaims`.
    #[serde(rename = "cognito:groups", default)]
    pub groups: Option<Vec<String>>,
    /// Cognito token kind: `"id"` for ID tokens, `"access"` for access tokens.
    /// The session path expects an ID token; an access token must not be
    /// accepted in its place.
    #[serde(default)]
    pub token_use: Option<String>,
    pub exp: i64,
}

/// OIDC client that handles discovery, code exchange, and JWT validation.
pub struct OidcClient {
    /// Browser-facing authorization endpoint.
    pub authorization_endpoint: String,
    /// Browser-facing end-session endpoint.
    pub end_session_endpoint: String,
    /// RFC 7009 revocation endpoint for backchannel logout (server-side only).
    revocation_endpoint: String,
    token_endpoint: String,
    client_id: String,
    client_secret: String,
    /// Expected `iss` claim — validated on every token (SECURITY: rejects
    /// tokens minted by a different issuer even if signed by a JWKS key).
    issuer: String,
    jwks: Arc<RwLock<Option<CachedJwks>>>,
    jwks_uri: String,
    http: reqwest::Client,
}

impl OidcClient {
    /// Discover OIDC endpoints from the issuer's well-known configuration.
    pub async fn discover(
        issuer: &str,
        client_id: String,
        client_secret: String,
    ) -> Result<Self, ApiError> {
        let http = reqwest::Client::new();
        let url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );

        let discovery: OidcDiscovery = http
            .get(&url)
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("OIDC discovery request failed: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::Internal(format!("OIDC discovery parse failed: {e}")))?;

        // Prefer the discovery doc's revocation_endpoint (RFC 7009).
        // Fall back to deriving it from the token endpoint for providers that
        // don't advertise it.
        let revocation_endpoint = discovery.revocation_endpoint.clone().unwrap_or_else(|| {
            discovery
                .token_endpoint
                .strip_suffix("/token")
                .map(|base| format!("{base}/revoke"))
                .unwrap_or_else(|| discovery.end_session_endpoint.clone())
        });

        tracing::info!(
            authorization_endpoint = %discovery.authorization_endpoint,
            token_endpoint = %discovery.token_endpoint,
            revocation_endpoint = %revocation_endpoint,
            jwks_uri = %discovery.jwks_uri,
            "OIDC discovery complete"
        );

        // Prefer the discovery doc's own issuer (authoritative — it is exactly
        // what the IdP puts in the `iss` claim); fall back to the configured one.
        let issuer = discovery
            .issuer
            .unwrap_or_else(|| issuer.trim_end_matches('/').to_string());

        Ok(Self {
            authorization_endpoint: discovery.authorization_endpoint,
            end_session_endpoint: discovery.end_session_endpoint,
            revocation_endpoint,
            token_endpoint: discovery.token_endpoint,
            client_id,
            client_secret,
            issuer,
            jwks: Arc::new(RwLock::new(None)),
            jwks_uri: discovery.jwks_uri,
            http,
        })
    }

    /// Exchange an authorization code for tokens.
    ///
    /// `code_verifier` must be supplied when the authorization request used PKCE
    /// (required for Cognito app clients created after November 2024).
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        code_verifier: Option<&str>,
    ) -> Result<TokenResponse, ApiError> {
        let mut params: Vec<(&str, &str)> = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];
        if let Some(cv) = code_verifier {
            params.push(("code_verifier", cv));
        }
        let resp = self
            .http
            .post(&self.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("token exchange request failed: {e}")))?;

        if !resp.status().is_success() {
            // 4xx from Cognito at the code-exchange step is a stale/missing PKCE
            // verifier or a replayed authorization code — an auth-flow failure,
            // not a server bug. Classify as Unauthorized so the callback handler
            // can redirect the user back into a fresh login flow rather than
            // emitting a 500 + JSON error page.
            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "unknown".to_string());
            tracing::warn!(%status, %body, "OIDC code exchange rejected");
            return Err(ApiError::Unauthorized(format!(
                "token exchange failed ({status}): {body}"
            )));
        }

        resp.json::<TokenResponse>()
            .await
            .map_err(|e| ApiError::Internal(format!("token response parse failed: {e}")))
    }

    /// Revoke the refresh token server-side (backchannel logout / RFC 7009).
    ///
    /// Errors are non-fatal: a stale/expired refresh token still means the
    /// user should be treated as logged out from our side.
    pub async fn backchannel_logout(&self, refresh_token: &str) {
        let result = self
            .http
            .post(&self.revocation_endpoint)
            .form(&[
                ("token", refresh_token),
                ("token_type_hint", "refresh_token"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
            ])
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("backchannel logout: refresh token revoked");
            }
            Ok(resp) => {
                let status = resp.status();
                tracing::warn!(
                    "backchannel logout: non-success status {status} \
                     (token may already be expired)"
                );
            }
            Err(e) => {
                tracing::warn!("backchannel logout: request failed: {e}");
            }
        }
    }

    /// Exchange a refresh token for a new set of tokens.
    pub async fn exchange_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<TokenResponse, ApiError> {
        let resp = self
            .http
            .post(&self.token_endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("refresh token request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "unknown".to_string());
            // Log the upstream detail server-side; return a generic message so
            // the IdP's error body isn't reflected to (unauthenticated) callers.
            tracing::warn!(%status, %body, "refresh token exchange failed");
            return Err(ApiError::Unauthorized(
                "could not refresh session".to_string(),
            ));
        }

        resp.json::<TokenResponse>()
            .await
            .map_err(|e| ApiError::Internal(format!("refresh token response parse failed: {e}")))
    }

    /// Validate a JWT (typically the ID token), returning decoded claims.
    ///
    /// On failure, a generic error is returned to the client while the
    /// detailed reason is logged server-side to avoid leaking implementation
    /// details (key IDs, audience mismatches, etc.).
    pub async fn validate_token(&self, token: &str) -> Result<OidcClaims, ApiError> {
        let jwks = self.get_jwks().await?;

        let header = jsonwebtoken::decode_header(token).map_err(|e| {
            tracing::warn!(%e, "JWT decode header failed");
            ApiError::Unauthorized("invalid token".to_string())
        })?;

        let kid = header
            .kid
            .as_deref()
            .ok_or_else(|| ApiError::Unauthorized("invalid token".to_string()))?;

        let jwk = jwks
            .keys
            .iter()
            .find(|k| k.common.key_id.as_deref() == Some(kid))
            .ok_or_else(|| {
                tracing::warn!(%kid, "JWT references unknown key ID");
                ApiError::Unauthorized("invalid token".to_string())
            })?;

        let decoding_key = DecodingKey::from_jwk(jwk).map_err(|e| {
            tracing::warn!(%e, %kid, "failed to derive decoding key from JWK");
            ApiError::Unauthorized("invalid token".to_string())
        })?;

        // Cognito ID tokens set `aud` to the App Client ID.
        // Validate it explicitly so tokens issued for other apps in the same
        // User Pool are rejected.
        // SECURITY: Always enforce RS256 — never trust the algorithm claim
        // from the untrusted token header.  Accepting `header.alg` would allow
        // an attacker to craft an HS256 token signed with the public key.
        let mut validation = Validation::new(jsonwebtoken::Algorithm::RS256);
        validation.set_audience(&[self.client_id.as_str()]);
        // SECURITY: pin the issuer so a token minted by a different issuer (even
        // one whose key happens to appear in the fetched JWKS) is rejected.
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.validate_exp = true;

        let token_data = jsonwebtoken::decode::<OidcClaims>(token, &decoding_key, &validation)
            .map_err(|e| {
                tracing::warn!(%e, %kid, "JWT validation failed");
                ApiError::Unauthorized("invalid token".to_string())
            })?;

        // SECURITY: this is the session path — reject an access token presented
        // in place of an ID token. Lenient: only an explicit "access" is
        // rejected (ID tokens carry "id"; tokens without the claim are allowed).
        if token_data.claims.token_use.as_deref() == Some("access") {
            tracing::warn!(%kid, "access token presented on the session path; rejected");
            return Err(ApiError::Unauthorized("invalid token".to_string()));
        }

        Ok(token_data.claims)
    }

    /// Change a user's password using their Cognito access token.
    ///
    /// Calls the Cognito Identity Provider service directly via HTTP — no AWS
    /// SDK dependency required.  The service URL is derived from the token
    /// endpoint (same AWS hostname, different path).
    ///
    /// Cognito-only: with any other issuer (e.g. the local Keycloak stack,
    /// deploy/docker-compose.local-auth.yml) this returns a clean Validation
    /// error instead of firing the AWS-proprietary call at a server that
    /// cannot understand it.
    pub async fn change_password(
        &self,
        access_token: &str,
        previous_password: &str,
        proposed_password: &str,
    ) -> Result<(), ApiError> {
        if !is_cognito_token_endpoint(&self.token_endpoint) {
            return Err(ApiError::Validation(
                "password changes are managed by your identity provider, not Ember Trove"
                    .to_string(),
            ));
        }
        // Derive the Cognito service root from the token_endpoint URL.
        // token_endpoint: https://cognito-idp.<region>.amazonaws.com/<pool>/oauth2/token
        // service root:   https://cognito-idp.<region>.amazonaws.com/
        let service_root = self
            .token_endpoint
            .splitn(4, '/')
            .take(3)
            .collect::<Vec<_>>()
            .join("/");

        let resp = self
            .http
            .post(&service_root)
            .header("Content-Type", "application/x-amz-json-1.1")
            .header(
                "X-Amz-Target",
                "AWSCognitoIdentityProviderService.ChangePassword",
            )
            .json(&serde_json::json!({
                "AccessToken": access_token,
                "PreviousPassword": previous_password,
                "ProposedPassword": proposed_password,
            }))
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("ChangePassword request failed: {e}")))?;

        if resp.status().is_success() {
            return Ok(());
        }

        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let upstream_msg = body
            .get("message")
            .or_else(|| body.get("Message"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let code = body.get("__type").and_then(|v| v.as_str()).unwrap_or("");

        // Log the provider's wording server-side; return a fixed client-facing
        // message per error class so Cognito's internal phrasing / policy detail
        // isn't reflected to the client.
        tracing::warn!(%status, %code, %upstream_msg, "ChangePassword failed");
        match code {
            "NotAuthorizedException" => Err(ApiError::Unauthorized(
                "current password is incorrect".to_string(),
            )),
            "InvalidPasswordException" => Err(ApiError::Validation(
                "new password does not meet the password policy".to_string(),
            )),
            "LimitExceededException" => Err(ApiError::Unauthorized(
                "too many attempts; please try again later".to_string(),
            )),
            _ => Err(ApiError::Internal("password change failed".to_string())),
        }
    }

    /// Fetch (and cache) the JWKS from the OIDC provider.
    async fn get_jwks(&self) -> Result<JwkSet, ApiError> {
        {
            let cached = self.jwks.read().await;
            if let Some(ref cached) = *cached
                && cached.fetched_at.elapsed() < JWKS_TTL
            {
                return Ok(cached.jwks.clone());
            }
        }

        let jwks: JwkSet = self
            .http
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("JWKS fetch failed: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::Internal(format!("JWKS parse failed: {e}")))?;

        let mut cached = self.jwks.write().await;
        *cached = Some(CachedJwks {
            jwks: jwks.clone(),
            fetched_at: Instant::now(),
        });

        Ok(jwks)
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

    #[test]
    fn cognito_endpoint_detection_is_host_suffix_based() {
        use super::is_cognito_token_endpoint as is_cognito;
        // Real Cognito shape.
        assert!(is_cognito(
            "https://cognito-idp.us-east-2.amazonaws.com/us-east-2_x/oauth2/token"
        ));
        // Local Keycloak (docker-compose.local-auth.yml) and generic IdPs.
        assert!(!is_cognito(
            "http://keycloak:8080/realms/ember-trove/protocol/openid-connect/token"
        ));
        assert!(!is_cognito("https://idp.example.com/oauth2/token"));
        // Host-suffix matching, not substring: a lookalike domain fails.
        assert!(!is_cognito("https://x.amazonaws.com.evil.example/token"));
        // Unparseable input fails closed.
        assert!(!is_cognito("not a url"));
    }

    // Test-only RSA-2048 PUBLIC key (public keys are not secrets). The matching
    // private key is deliberately NOT committed — this test never signs a token,
    // it only drives the verification path, which is where the bug lived.
    const TEST_RSA_PUB_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAov6rpx+ruypmo8bkKtxu
MCMnH3mMXKa9u0/u2LwDb4TB1z/R65tRZiQVYMWbZxe7OVwMFRcfJHuEhfbWQZBV
2e1uBtLmOstDF6HiIpsGdBS8SBuZ/qfX+njqURPms9VyRXZFtTWtf1IZJx15oWG/
1dUxv0SdLQjosg2BonoTRwk8ZuySH0qVsvE+ypWmzqXFST/bojgFb2ugQoUNoBvi
UqCJ2b9bkp23/Mwzx298nz6EUoKFcKghsnh2sx2cbdk+rzQzttnYnYMV26kIBrea
ebZu1XCPdvwiw7+hEr99Q4BndQbtYIOXPeyargCFe2YeKYOC80X6bNj0EZwJBtcR
2QIDAQAB
-----END PUBLIC KEY-----"#;

    /// Regression test for the production login outage (502 on every authenticated
    /// request). jsonwebtoken 10 ships no built-in crypto backend; with neither
    /// `aws_lc_rs` nor `rust_crypto` enabled, the FIRST RS256 verification panics
    /// at runtime ("Could not automatically determine the process-level
    /// CryptoProvider …"), crashing the worker. Decoding ANY RS256 token invokes
    /// the provider's verifier, so this hits the same path: it panics without a
    /// backend, and cleanly rejects the bogus signature (`InvalidSignature`) with
    /// `aws_lc_rs`. The `oidc = None` middleware tests never reach this path.
    #[test]
    fn rs256_verification_runs_without_panicking() {
        // A structurally valid RS256 JWT with a bogus (all-zero) signature.
        let header = B64.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = B64.encode(br#"{"sub":"x","exp":4102444800}"#); // exp 2100-01-01
        let sig = B64.encode([0u8; 256]); // RSA-2048 signature width
        let token = format!("{header}.{claims}.{sig}");

        let dec = DecodingKey::from_rsa_pem(TEST_RSA_PUB_PEM.as_bytes()).unwrap();
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false;

        // Without a crypto backend this panics resolving the CryptoProvider; with
        // `aws_lc_rs` the provider runs the RSA verification and rejects the bogus
        // signature. Either way the point is: it returns, it does not panic.
        let result = decode::<serde_json::Value>(&token, &dec, &validation);
        assert!(
            matches!(
                result,
                Err(ref e) if *e.kind() == jsonwebtoken::errors::ErrorKind::InvalidSignature
            ),
            "expected InvalidSignature (crypto provider ran), got {result:?}"
        );
    }
}
