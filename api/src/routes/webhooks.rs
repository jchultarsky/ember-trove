use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use common::{
    auth::AuthClaims,
    id::WebhookId,
    webhook::{CreateWebhookRequest, UpdateWebhookRequest, Webhook},
};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

/// True if `ip` is loopback / private / link-local / IMDS / unspecified — i.e.
/// a target a webhook must never reach (SSRF).
fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254)
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 (ULA)
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 (link-local)
        }
    }
}

/// Reject webhook URLs targeting private/internal networks (SSRF prevention).
/// Synchronous structural checks (scheme, literal IPs, internal hostnames).
/// DNS resolution is handled separately by [`validate_webhook_dns`].
fn validate_webhook_url(url: &str) -> Result<(), ApiError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ApiError::Validation("invalid webhook URL".to_string()))?;

    // Only allow HTTPS (and HTTP for localhost dev).
    match parsed.scheme() {
        "https" => {}
        "http" => {
            // Allow http only for explicit localhost in dev.
            let host = parsed.host_str().unwrap_or("");
            if host != "localhost" && host != "127.0.0.1" {
                return Err(ApiError::Validation(
                    "webhook URL must use HTTPS".to_string(),
                ));
            }
        }
        _ => {
            return Err(ApiError::Validation(
                "webhook URL must use HTTPS".to_string(),
            ));
        }
    }

    // Block private/link-local IP ranges given as a literal host.
    if let Some(host) = parsed.host_str() {
        if let Ok(ip) = host.parse::<std::net::IpAddr>()
            && is_blocked_ip(ip)
        {
            return Err(ApiError::Validation(
                "webhook URL must not target private networks".to_string(),
            ));
        }
        // Block AWS/GCP metadata endpoint by hostname.
        let lower = host.to_lowercase();
        if lower == "metadata.google.internal"
            || lower.ends_with(".internal")
            || lower == "instance-data"
        {
            return Err(ApiError::Validation(
                "webhook URL must not target internal services".to_string(),
            ));
        }
    }

    Ok(())
}

/// Resolve the webhook host and reject if ANY resolved IP is private/internal.
/// Closes the SSRF bypass where a public hostname resolves to a private/IMDS
/// address. (Does not defeat DNS-rebinding TOCTOU; dispatch also disables
/// redirects as a second layer.)
async fn validate_webhook_dns(url: &str) -> Result<(), ApiError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| ApiError::Validation("invalid webhook URL".to_string()))?;
    let Some(host) = parsed.host_str() else {
        return Ok(());
    };
    // Literal IPs were already checked synchronously.
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| ApiError::Validation("webhook host does not resolve".to_string()))?;
    for addr in addrs {
        if is_blocked_ip(addr.ip()) {
            return Err(ApiError::Validation(
                "webhook URL resolves to a private/internal address".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_webhooks).post(create_webhook))
        .route(
            "/{id}",
            axum::routing::put(update_webhook).delete(delete_webhook),
        )
}

/// Mask the webhook secret so it's never returned in full after creation.
fn mask_secret(mut hook: Webhook) -> Webhook {
    if let Some(ref s) = hook.secret {
        hook.secret = Some(mask_value(s));
    }
    hook
}

/// Mask a secret string, revealing at most its first 4 characters.
///
/// Char-safe: byte-slicing `&s[..4]` panics if the 4th byte falls mid-UTF-8
/// (e.g. a secret beginning with a multi-byte character).
fn mask_value(s: &str) -> String {
    if s.chars().count() > 4 {
        let prefix: String = s.chars().take(4).collect();
        format!("{prefix}…")
    } else {
        "••••".to_string()
    }
}

async fn list_webhooks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<Webhook>>, ApiError> {
    let hooks: Vec<Webhook> = state
        .webhooks
        .list(&claims.sub)
        .await?
        .into_iter()
        .map(mask_secret)
        .collect();
    Ok(Json(hooks))
}

async fn create_webhook(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<Webhook>), ApiError> {
    validate_webhook_url(&req.url)?;
    validate_webhook_dns(&req.url).await?;
    let hook = state.webhooks.create(&claims.sub, req).await?;
    // Mask the secret in the response too (consistent with list/update); the
    // client supplied it, so nothing is lost.
    Ok((StatusCode::CREATED, Json(mask_secret(hook))))
}

async fn update_webhook(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateWebhookRequest>,
) -> Result<Json<Webhook>, ApiError> {
    validate_webhook_url(&req.url)?;
    validate_webhook_dns(&req.url).await?;
    let hook = state
        .webhooks
        .update(WebhookId(id), &claims.sub, req)
        .await?;
    Ok(Json(mask_secret(hook)))
}

async fn delete_webhook(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.webhooks.delete(WebhookId(id), &claims.sub).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::{is_blocked_ip, mask_value, validate_webhook_url};

    #[test]
    fn is_blocked_ip_flags_private_and_imds() {
        use std::net::IpAddr;
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254", // AWS IMDS
            "0.0.0.0",
        ] {
            assert!(
                is_blocked_ip(ip.parse::<IpAddr>().unwrap()),
                "{ip} should be blocked"
            );
        }
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(
                !is_blocked_ip(ip.parse::<IpAddr>().unwrap()),
                "{ip} should be allowed"
            );
        }
    }

    #[test]
    fn mask_value_is_char_safe_on_multibyte_secret() {
        // Regression: byte-slicing `&s[..4]` would panic here (the 4th byte is
        // mid-codepoint). Must reveal the first 4 chars without panicking.
        assert_eq!(mask_value("🔑🔑🔑🔑🔑secret"), "🔑🔑🔑🔑…");
        assert_eq!(mask_value("ünîçødë-key"), "ünîç…");
    }

    #[test]
    fn mask_value_hides_short_secrets_entirely() {
        assert_eq!(mask_value("abcd"), "••••");
        assert_eq!(mask_value("🔑"), "••••");
        assert_eq!(mask_value(""), "••••");
    }

    #[test]
    fn mask_value_reveals_prefix_of_long_ascii() {
        assert_eq!(mask_value("supersecret"), "supe…");
    }

    #[test]
    fn validate_webhook_url_rejects_plaintext_http_and_private() {
        assert!(validate_webhook_url("https://example.com/hook").is_ok());
        assert!(validate_webhook_url("http://example.com/hook").is_err());
        assert!(validate_webhook_url("http://localhost:9000/h").is_ok());
        assert!(validate_webhook_url("https://169.254.169.254/").is_err());
        assert!(validate_webhook_url("https://10.0.0.5/").is_err());
        assert!(validate_webhook_url("ftp://example.com/").is_err());
    }
}
