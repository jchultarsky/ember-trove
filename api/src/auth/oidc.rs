use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::ApiError;

/// TTL for JWKS cache — refresh after 1 hour to handle key rotation.
const JWKS_TTL: Duration = Duration::from_secs(3600);

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
    pub async fn change_password(
        &self,
        access_token: &str,
        previous_password: &str,
        proposed_password: &str,
    ) -> Result<(), ApiError> {
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
    use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
    use serde::{Deserialize, Serialize};

    // Test-only RSA-2048 keypair (NOT a secret — generated for this test alone).
    const TEST_RSA_PRIV_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCi/qunH6u7Kmaj
xuQq3G4wIycfeYxcpr27T+7YvANvhMHXP9Hrm1FmJBVgxZtnF7s5XAwVFx8ke4SF
9tZBkFXZ7W4G0uY6y0MXoeIimwZ0FLxIG5n+p9f6eOpRE+az1XJFdkW1Na1/Uhkn
HXmhYb/V1TG/RJ0tCOiyDYGiehNHCTxm7JIfSpWy8T7KlabOpcVJP9uiOAVva6BC
hQ2gG+JSoInZv1uSnbf8zDPHb3yfPoRSgoVwqCGyeHazHZxt2T6vNDO22didgxXb
qQgGt5p5tm7VcI92/CLDv6ESv31DgGd1Bu1gg5c97JquAIV7Zh4pg4LzRfps2PQR
nAkG1xHZAgMBAAECggEAGBfDWTh/+rJFijNpwiEjo3dqvscJdZ+K/49M65n8+wS5
X18WYThr44h1ZYDIHAjAurWNrqdkidC9Mc0e9gGHAyfKofUWJX4qnGloIvvdzBZN
j24PCPqX3PT3E8V4jkAAfF6DZsn4q49/2s2LT0zC3bF+ATr77a55sOn4rcLUKyVo
s8wrHK0WL1pfNHpI/bCzxEf5XHB+PhIu77/NXm3EiNdeG8ppi2ozakv1FpUom9j3
4hkWIYMZG27LHqB79gKA09cRJgit5QhOLoqgM4fdFKiyPz5LDpZesh7URcQDmPks
aO0YuZL/m6N6sqhK0+aWT/6laIccBDpWMuCmb255NQKBgQDPshS1ukjgK5WwDc2w
ZVAN/43DgAYiAGkwPqmDQVEjOm5pOx2Ojzg3Quo1U3f/MhxryrpDqZzahV4CjNRM
xJs3BSLRE3mZmt1LxLKhCFNeomW3Q7qu2IzYim5D3RN2S4xnTeFGNEf6PihRvvkC
bLVxyH1jOrlT7g3Tb+mCSpylhQKBgQDI5ymj2/Qi21HYJBbQO4GVx48Fxqurijex
fr21X4+0PKkMRhAn1RxRtPN1Uyvr8RQfkkEqUJZ/CMvEd+bexG3zusx1bm2t5Zsx
fnXYIMOaoCz7/Sw2wlmqY+CE62fuBbUik9AIp9SLj4LlmXr+/8ZN7baJV80z13b+
e6JOLjAxRQKBgQDA5iCd9/ofWduYu/lBz5beqW89F/aaNc98Y3aE1XFKSsapLaJx
+Uq46IkmJfPZLO2An7UHisyHmD6MF8hF1IRkQXzoujHCHDdUW8ecEGN+DU5zO5Bz
O+T0aP2oQfgFcn1gpNCJp50CKiDAa6JSQizzFMaAFtZxwTNOIS67OBjtEQKBgQCb
lsMd3uOE9zu8W767R8qE+Abg30rmT+Xv9YrwY3DEklINallqr9X9xVjjHSWf1ZXT
GY6UOdND0MkWgBFxpsjMgHeF3p7clTyKqTiUyFMUdkZAZYMPaZbNqgoghrt3kD4G
6Fity2SFLQCf1ix2PhoTEi1S0ofeRVknnxJE3+p8zQKBgBZlwZS9ewAcBueVWWSF
K/9lYzH/RigDpLzMT2lRkpqarw56dzFoVQ6k7WOXozVNcBGKNTqC3iUQ3dpU4SzS
APpA95rdkf54lY5qea5ug9fHBRHm8J2efkwYCWkBnVje47a77HLR2kzMPACV+cri
V5o6nNkpx+i99wYZxvFU0IKl
-----END PRIVATE KEY-----"#;

    const TEST_RSA_PUB_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAov6rpx+ruypmo8bkKtxu
MCMnH3mMXKa9u0/u2LwDb4TB1z/R65tRZiQVYMWbZxe7OVwMFRcfJHuEhfbWQZBV
2e1uBtLmOstDF6HiIpsGdBS8SBuZ/qfX+njqURPms9VyRXZFtTWtf1IZJx15oWG/
1dUxv0SdLQjosg2BonoTRwk8ZuySH0qVsvE+ypWmzqXFST/bojgFb2ugQoUNoBvi
UqCJ2b9bkp23/Mwzx298nz6EUoKFcKghsnh2sx2cbdk+rzQzttnYnYMV26kIBrea
ebZu1XCPdvwiw7+hEr99Q4BndQbtYIOXPeyargCFe2YeKYOC80X6bNj0EZwJBtcR
2QIDAQAB
-----END PUBLIC KEY-----"#;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Claims {
        sub: String,
        exp: usize,
    }

    /// Regression test for the production login outage: jsonwebtoken 10 requires
    /// an explicit crypto-backend feature (`aws_lc_rs` or `rust_crypto`). With
    /// neither enabled, the FIRST RS256 verification panics at runtime
    /// ("Could not automatically determine the process-level CryptoProvider …"),
    /// crashing the worker → nginx 502 on every authenticated request. Exercising
    /// a real RS256 sign+verify here fails (panics) unless a backend is enabled,
    /// so this guards the `aws_lc_rs` feature against being dropped again. The
    /// `oidc = None` middleware tests never reach this path.
    #[test]
    fn rs256_sign_and_verify_roundtrip_does_not_panic() {
        let claims = Claims {
            sub: "user-123".to_string(),
            // far-future expiry so `validate_exp` passes deterministically
            exp: 4_102_444_800, // 2100-01-01
        };

        let enc = EncodingKey::from_rsa_pem(TEST_RSA_PRIV_PEM.as_bytes()).unwrap();
        let token = encode(&Header::new(Algorithm::RS256), &claims, &enc).unwrap();

        let dec = DecodingKey::from_rsa_pem(TEST_RSA_PUB_PEM.as_bytes()).unwrap();
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false; // no `aud` claim in this minimal token

        let decoded = decode::<Claims>(&token, &dec, &validation).unwrap();
        assert_eq!(decoded.claims, claims);
    }
}
