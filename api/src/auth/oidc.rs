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

/// Minimum spacing between JWKS refetches triggered by an unknown `kid`.
/// The `kid` comes from an unauthenticated token header, so without this a
/// stream of forged kids would turn every request into an upstream fetch.
const JWKS_UNKNOWN_KID_COOLDOWN: Duration = Duration::from_secs(60);

/// Cached JWKS with timestamps for TTL invalidation and refetch rate-limiting.
struct CachedJwks {
    jwks: JwkSet,
    fetched_at: Instant,
    /// Last unknown-`kid` refetch attempt, successful or not — the cooldown
    /// anchor. Set to `fetched_at` on every successful fetch.
    last_forced_attempt: Instant,
}

/// What `get_jwks` should do for a given `kid`; see [`jwks_action`].
#[derive(Debug, PartialEq, Eq)]
enum JwksAction {
    /// The cached set is fresh and either has the `kid` or is inside the
    /// unknown-`kid` cooldown — use it as is.
    UseCached,
    /// Nothing cached or the TTL lapsed — fetch; a failure is an error.
    Fetch,
    /// The cache is fresh but lacks the `kid` (likely a key rotation) and the
    /// cooldown has elapsed — refetch once; on failure keep the cached set.
    RefreshForUnknownKid,
}

/// Pure JWKS cache policy: TTL refresh plus a rate-limited refetch on an
/// unknown `kid`, so Cognito key rotation is picked up immediately rather than
/// failing logins for up to `JWKS_TTL`.
fn jwks_action(cached: Option<&CachedJwks>, kid: &str, now: Instant) -> JwksAction {
    let Some(c) = cached else {
        return JwksAction::Fetch;
    };
    if now.saturating_duration_since(c.fetched_at) >= JWKS_TTL {
        return JwksAction::Fetch;
    }
    if c.jwks.find(kid).is_some()
        || now.saturating_duration_since(c.last_forced_attempt) < JWKS_UNKNOWN_KID_COOLDOWN
    {
        return JwksAction::UseCached;
    }
    JwksAction::RefreshForUnknownKid
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
        let header = jsonwebtoken::decode_header(token).map_err(|e| {
            tracing::warn!(%e, "JWT decode header failed");
            ApiError::Unauthorized("invalid token".to_string())
        })?;

        let kid = header
            .kid
            .as_deref()
            .ok_or_else(|| ApiError::Unauthorized("invalid token".to_string()))?;

        let jwks = self.get_jwks(kid).await?;

        let jwk = jwks.find(kid).ok_or_else(|| {
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

    /// Return the JWKS to verify a token signed with `kid`, fetching from the
    /// provider per [`jwks_action`] (TTL expiry, or a rate-limited refetch when
    /// the `kid` is unknown — Cognito key rotation).
    async fn get_jwks(&self, kid: &str) -> Result<JwkSet, ApiError> {
        let action = {
            let cached = self.jwks.read().await;
            let action = jwks_action(cached.as_ref(), kid, Instant::now());
            if let (JwksAction::UseCached, Some(c)) = (&action, cached.as_ref()) {
                return Ok(c.jwks.clone());
            }
            action
        };

        if action == JwksAction::RefreshForUnknownKid {
            // Claim the attempt under the write lock (re-checking, since another
            // request may have just refreshed) so concurrent requests don't each
            // hit the IdP — without holding the lock across the network call.
            let fallback = {
                let mut guard = self.jwks.write().await;
                let now = Instant::now();
                match guard.as_mut() {
                    Some(c) if jwks_action(Some(c), kid, now) == action => {
                        c.last_forced_attempt = now;
                        c.jwks.clone()
                    }
                    Some(c) => return Ok(c.jwks.clone()),
                    None => return self.fetch_and_cache_jwks().await,
                }
            };
            // A failed rotation refetch must not take auth down: keep serving
            // the still-valid cached set (the unknown kid is then rejected).
            return match self.fetch_and_cache_jwks().await {
                Ok(jwks) => Ok(jwks),
                Err(e) => {
                    tracing::warn!(%e, "JWKS refetch for unknown kid failed; using cached set");
                    Ok(fallback)
                }
            };
        }

        self.fetch_and_cache_jwks().await
    }

    /// Fetch the JWKS from the provider and replace the cache.
    async fn fetch_and_cache_jwks(&self) -> Result<JwkSet, ApiError> {
        let jwks: JwkSet = self
            .http
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("JWKS fetch failed: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::Internal(format!("JWKS parse failed: {e}")))?;

        let now = Instant::now();
        let mut cached = self.jwks.write().await;
        *cached = Some(CachedJwks {
            jwks: jwks.clone(),
            fetched_at: now,
            last_forced_attempt: now,
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

    /// Cognito's additive token-claim rollout (from 2026-11-04) adds `acr` and
    /// an array-valued `amr`. Claim parsing must ignore claims it does not read
    /// — a `deny_unknown_fields` on `OidcClaims` would lock every user out.
    #[test]
    fn claims_tolerate_additive_cognito_claims() {
        let payload = serde_json::json!({
            "sub": "a1b2c3d4-5678-90ab-cdef-EXAMPLE11111",
            "iss": "https://cognito-idp.us-east-2.amazonaws.com/us-east-2_x",
            "aud": "client",
            "token_use": "id",
            "email": "jane@example.com",
            "cognito:groups": ["admin"],
            "iat": 1_790_000_000,
            "exp": 1_790_003_600,
            "acr": "urn:cognito:loa:4",
            "amr": ["pwd", "otp", "mfa"],
            "some_future_claim": { "nested": [1, 2, 3] }
        });
        let claims: super::OidcClaims =
            serde_json::from_value(payload).expect("additive claims must not break parsing");
        assert_eq!(claims.sub, "a1b2c3d4-5678-90ab-cdef-EXAMPLE11111");
        assert_eq!(claims.token_use.as_deref(), Some("id"));
        assert_eq!(claims.groups, Some(vec!["admin".to_string()]));
        assert_eq!(claims.exp, 1_790_003_600);
    }

    /// Same contract for `/.well-known/openid-configuration`: new optional
    /// members (`acr_values_supported`, `code_challenge_methods_supported`,
    /// `grant_types_supported`, …) must not fail discovery at startup.
    #[test]
    fn discovery_tolerates_additive_metadata() {
        let doc = serde_json::json!({
            "issuer": "https://cognito-idp.us-east-2.amazonaws.com/us-east-2_x",
            "authorization_endpoint": "https://auth.example/oauth2/authorize",
            "token_endpoint": "https://auth.example/oauth2/token",
            "jwks_uri": "https://cognito-idp.us-east-2.amazonaws.com/us-east-2_x/.well-known/jwks.json",
            "end_session_endpoint": "https://auth.example/logout",
            "revocation_endpoint": "https://auth.example/oauth2/revoke",
            "acr_values_supported": ["urn:cognito:loa:1", "urn:cognito:loa:4"],
            "code_challenge_methods_supported": ["S256"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "some_future_member": { "x": true }
        });
        let d: super::OidcDiscovery =
            serde_json::from_value(doc).expect("additive metadata must not break discovery");
        assert_eq!(d.token_endpoint, "https://auth.example/oauth2/token");
        assert!(d.jwks_uri.ends_with("/.well-known/jwks.json"));
    }

    fn cached_with_kid(kid: &str, at: std::time::Instant) -> super::CachedJwks {
        let jwks = serde_json::from_value(serde_json::json!({
            "keys": [{ "kty": "RSA", "kid": kid, "alg": "RS256", "use": "sig",
                       "n": "AQAB", "e": "AQAB" }]
        }))
        .unwrap();
        super::CachedJwks {
            jwks,
            fetched_at: at,
            last_forced_attempt: at,
        }
    }

    #[test]
    fn jwks_action_fetches_when_nothing_cached() {
        use super::{JwksAction, jwks_action};
        let now = std::time::Instant::now();
        assert_eq!(jwks_action(None, "k1", now), JwksAction::Fetch);
    }

    #[test]
    fn jwks_action_uses_fresh_cache_for_known_kid() {
        use super::{JwksAction, jwks_action};
        let t0 = std::time::Instant::now();
        let c = cached_with_kid("k1", t0);
        let later = t0 + std::time::Duration::from_secs(600);
        assert_eq!(jwks_action(Some(&c), "k1", later), JwksAction::UseCached);
    }

    #[test]
    fn jwks_action_refetches_after_ttl_even_for_known_kid() {
        use super::{JWKS_TTL, JwksAction, jwks_action};
        let t0 = std::time::Instant::now();
        let c = cached_with_kid("k1", t0);
        assert_eq!(
            jwks_action(Some(&c), "k1", t0 + JWKS_TTL),
            JwksAction::Fetch
        );
    }

    /// Key rotation: a token signed with a kid we have not seen yet triggers one
    /// refetch instead of failing every login until the hourly TTL lapses.
    #[test]
    fn jwks_action_refreshes_for_unknown_kid_after_cooldown() {
        use super::{JWKS_UNKNOWN_KID_COOLDOWN, JwksAction, jwks_action};
        let t0 = std::time::Instant::now();
        let c = cached_with_kid("k1", t0);
        assert_eq!(
            jwks_action(Some(&c), "k2", t0 + JWKS_UNKNOWN_KID_COOLDOWN),
            JwksAction::RefreshForUnknownKid
        );
    }

    /// SECURITY: the kid comes from an unauthenticated header, so a stream of
    /// forged kids must not turn every request into an upstream JWKS fetch.
    #[test]
    fn jwks_action_rate_limits_unknown_kid_refetches() {
        use super::{JWKS_UNKNOWN_KID_COOLDOWN, JwksAction, jwks_action};
        let t0 = std::time::Instant::now();
        let mut c = cached_with_kid("k1", t0);
        // Just after a fetch: inside the cooldown.
        let soon = t0 + JWKS_UNKNOWN_KID_COOLDOWN - std::time::Duration::from_secs(1);
        assert_eq!(jwks_action(Some(&c), "forged", soon), JwksAction::UseCached);
        // A forced attempt (even a failed one) restarts the cooldown.
        let t1 = t0 + JWKS_UNKNOWN_KID_COOLDOWN;
        c.last_forced_attempt = t1;
        assert_eq!(
            jwks_action(Some(&c), "forged", t1 + std::time::Duration::from_secs(1)),
            JwksAction::UseCached
        );
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
