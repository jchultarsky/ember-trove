//! Fire-and-forget webhook delivery.
//!
//! After a node or task mutation, call [`dispatch`] with the event name and
//! optional node ID.  Active webhooks subscribed to that event are fetched from
//! the repo, scoped to the resource owner, and an HTTP POST is spawned for each
//! one.  Delivery failures are logged but never propagate back to the caller.
//!
//! Wired into the node/task CRUD handlers (`routes/nodes.rs`, `routes/tasks.rs`)
//! using the canonical event-name constants in `common::webhook`.

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

        for hook in hooks {
            let (event, triggered_by) = (event.clone(), triggered_by.clone());
            // One task per hook, so a slow endpoint holds up only itself.
            // Each waits for a delivery slot first: see `DELIVERY_SLOTS`.
            tokio::spawn(async move {
                let wid = hook.id;
                let delivered = in_a_slot(&DELIVERY_SLOTS, SLOT_WAIT, async move {
                    // The POST has its own timeout; this one also covers the
                    // DNS lookup before it, so a slot always comes back.
                    let work = deliver(hook, &event, node_id, &triggered_by);
                    if tokio::time::timeout(DELIVERY_LIMIT, work).await.is_err() {
                        warn!("webhook POST (id={wid}) gave up: it took too long");
                    }
                })
                .await;
                if !delivered {
                    warn!("webhook POST (id={wid}) dropped: deliveries are backed up");
                }
            });
        }
    });
}

/// How many webhook deliveries may be in flight at once, across every
/// account. Each one holds a DNS lookup, a TLS connection and a client.
///
/// SECURITY: deliveries used to be spawned without limit, one per hook per
/// event. With hooks capped per account that is bounded per person, but
/// not across accounts or a burst of edits; this bounds the sockets and the
/// CPU the server will spend on other hosts at any moment (2026-11-10).
const MAX_IN_FLIGHT: usize = 32;
static DELIVERY_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_IN_FLIGHT);

/// How long a delivery waits for a slot before it is dropped. Delivery is
/// best effort and was never retried; a backlog older than this is dropped
/// rather than kept growing in memory.
const SLOT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// The longest one delivery may hold a slot: its DNS lookup plus the POST's
/// own 10 s.
const DELIVERY_LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Run `work` once a slot is free. `false` when none came free within
/// `wait`: the work was not run.
async fn in_a_slot<F>(slots: &tokio::sync::Semaphore, wait: std::time::Duration, work: F) -> bool
where
    F: std::future::Future<Output = ()>,
{
    // A closed semaphore never happens here (it is never closed); treat it
    // like no slot.
    let Ok(Ok(_slot)) = tokio::time::timeout(wait, slots.acquire()).await else {
        return false;
    };
    work.await;
    true
}

/// Vet, sign and POST one webhook. Failures are logged, never returned.
async fn deliver(
    hook: common::webhook::Webhook,
    event: &str,
    node_id: Option<NodeId>,
    triggered_by: &str,
) {
    // SECURITY: re-vet the URL at dispatch time, not just at
    // create/update. A hostname vetted at creation can be re-pointed
    // at a private/IMDS address later (DNS rebinding); resolving here
    // and pinning the client to the vetted addresses closes that
    // TOCTOU — the connection can only reach what we just checked.
    let target = match crate::ssrf::vet_url_for_dispatch(&hook.url).await {
        Ok(t) => t,
        Err(reason) => {
            warn!("webhook {} skipped (SSRF guard): {reason}", hook.id);
            return;
        }
    };

    // Per-hook client so the DNS pin is scoped to this hook's host.
    // Timeout: a slow endpoint must not pin the tokio task. Redirects
    // are disabled so a 30x to a private/IMDS address can't be
    // followed (second SSRF layer).
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if let crate::ssrf::DispatchTarget::Pinned {
        ref host,
        ref addrs,
    } = target
    {
        builder = builder.resolve_to_addrs(host, addrs);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => {
            warn!("webhook dispatch: reqwest client build failed: {e}");
            return;
        }
    };
    let payload = WebhookPayload {
        event: event.to_string(),
        webhook_id: hook.id,
        node_id,
        triggered_by: triggered_by.to_string(),
        timestamp: Utc::now(),
    };
    let body = match serde_json::to_string(&payload) {
        Ok(b) => b,
        Err(e) => {
            warn!("webhook serialize failed for {}: {e}", hook.id);
            return;
        }
    };

    let mut req = client
        .post(&hook.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Event", event);

    // HMAC-SHA256 signature if a secret is configured.
    if let Some(ref secret) = hook.secret
        && let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes())
    {
        mac.update(body.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        req = req.header("X-Webhook-Signature", format!("sha256={sig}"));
    }

    if let Err(e) = req.body(body).send().await {
        // SECURITY: never log the URL — Slack/Discord-style
        // webhook URLs carry their secret in the path, and
        // reqwest's error Display repeats it (`without_url`).
        warn!("webhook POST (id={}) failed: {}", hook.id, e.without_url());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use tokio::sync::Semaphore;

    use super::in_a_slot;

    #[tokio::test]
    async fn work_runs_when_a_slot_is_free_and_gives_it_back() {
        let slots = Semaphore::new(1);
        let ran = AtomicU32::new(0);
        for _ in 0..3 {
            assert!(
                in_a_slot(&slots, Duration::from_millis(50), async {
                    ran.fetch_add(1, Ordering::SeqCst);
                })
                .await
            );
        }
        assert_eq!(ran.load(Ordering::SeqCst), 3);
        assert_eq!(slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn work_is_dropped_unrun_when_no_slot_comes_free_in_time() {
        let slots = Semaphore::new(1);
        let held = slots.acquire().await.unwrap();
        let ran = AtomicU32::new(0);
        assert!(
            !in_a_slot(&slots, Duration::from_millis(20), async {
                ran.fetch_add(1, Ordering::SeqCst);
            })
            .await
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0, "the dropped work never ran");
        drop(held);
        assert!(in_a_slot(&slots, Duration::from_millis(20), async {}).await);
    }

    #[tokio::test]
    async fn no_more_than_the_slots_run_at_once() {
        let slots = std::sync::Arc::new(Semaphore::new(2));
        let now = std::sync::Arc::new(AtomicU32::new(0));
        let most = std::sync::Arc::new(AtomicU32::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let (slots, now, most) = (slots.clone(), now.clone(), most.clone());
            tasks.push(tokio::spawn(async move {
                in_a_slot(&slots, Duration::from_secs(5), async {
                    let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(n, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    now.fetch_sub(1, Ordering::SeqCst);
                })
                .await
            }));
        }
        for t in tasks {
            assert!(t.await.unwrap());
        }
        assert_eq!(most.load(Ordering::SeqCst), 2);
    }
}
