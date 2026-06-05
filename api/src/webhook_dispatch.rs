//! Fire-and-forget webhook delivery.
//!
//! After a node mutation, call [`dispatch`] with the event name and optional
//! node ID.  Active webhooks subscribed to that event are fetched from the repo
//! and an HTTP POST is spawned for each one.  Delivery failures are logged but
//! never propagate back to the caller.
//!
//! NOTE (2026-06-05): the delivery path is built and tenant-scoped but not yet
//! wired to any mutation handler — [`dispatch`] currently has no callers, so
//! registered webhooks do not fire. Tracked in `.claude/ROADMAP.md` (wire
//! webhook delivery into the node/task mutation routes). The module-scoped
//! `allow(dead_code)` keeps this retained-but-unwired code from tripping the
//! crate-wide dead-code lint, without a blanket relaxation across the crate.
#![allow(dead_code)]

use std::sync::Arc;

use chrono::Utc;
use common::{id::NodeId, webhook::WebhookPayload};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tracing::warn;

use crate::repo::webhook::WebhookRepo;

type HmacSha256 = Hmac<Sha256>;

/// Spawn fire-and-forget tasks that POST webhook payloads to subscribers.
///
/// SECURITY: only delivers to webhooks owned by `owner_id` (the owner of the
/// resource that triggered the event), so one tenant's events never fan out to
/// another tenant's endpoint (cross-tenant metadata leak).
pub fn dispatch(
    webhooks: Arc<dyn WebhookRepo>,
    event: &str,
    node_id: Option<NodeId>,
    triggered_by: &str,
    owner_id: &str,
) {
    let event = event.to_string();
    let triggered_by = triggered_by.to_string();
    let owner_id = owner_id.to_string();
    tokio::spawn(async move {
        let mut hooks = match webhooks.list_active_for_event(&event).await {
            Ok(h) => h,
            Err(e) => {
                warn!("webhook list_active_for_event failed: {e}");
                return;
            }
        };
        // Only fan out to the resource owner's own webhooks.
        hooks.retain(|h| h.owner_id == owner_id);
        // If client construction fails (e.g. TLS backend init error) abort
        // the whole dispatch rather than silently falling back to a client
        // with no timeout — a slow webhook endpoint on the default client
        // would pin the tokio task indefinitely. Redirects are disabled so a
        // 30x to a private/IMDS address can't be followed (SSRF).
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!("webhook dispatch: reqwest client build failed: {e}");
                return;
            }
        };

        for hook in hooks {
            let payload = WebhookPayload {
                event: event.clone(),
                webhook_id: hook.id,
                node_id,
                triggered_by: triggered_by.clone(),
                timestamp: Utc::now(),
            };
            let body = match serde_json::to_string(&payload) {
                Ok(b) => b,
                Err(e) => {
                    warn!("webhook serialize failed for {}: {e}", hook.id);
                    continue;
                }
            };

            let mut req = client
                .post(&hook.url)
                .header("Content-Type", "application/json")
                .header("X-Webhook-Event", &event);

            // HMAC-SHA256 signature if a secret is configured.
            if let Some(ref secret) = hook.secret
                && let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes())
            {
                mac.update(body.as_bytes());
                let sig = hex::encode(mac.finalize().into_bytes());
                req = req.header("X-Webhook-Signature", format!("sha256={sig}"));
            }

            let url = hook.url.clone();
            let wid = hook.id;
            tokio::spawn(async move {
                if let Err(e) = req.body(body).send().await {
                    warn!("webhook POST to {url} (id={wid}) failed: {e}");
                }
            });
        }
    });
}
