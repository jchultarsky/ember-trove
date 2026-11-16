//! Fire-and-forget webhook delivery.
//!
//! After a node mutation, call [`dispatch`]; after a task mutation,
//! [`dispatch_task_event`]. The active webhooks of the one account the
//! event is for, subscribed to that event, are fetched from the repo, and
//! an HTTP POST is spawned for each. Delivery failures are logged but
//! never propagate back to the caller.
//!
//! Wired into the node/task CRUD handlers (`routes/nodes.rs`, `routes/tasks.rs`)
//! using the canonical event-name constants in `common::webhook`.
//!
//! A node event goes to the hooks of the node's owner ([`dispatch`]). A
//! task event goes to the hooks of the task's author, and only while that
//! person can still see the task's node ([`dispatch_task_event`]).

use std::sync::Arc;

use chrono::Utc;
use common::{id::NodeId, webhook::WebhookPayload};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tracing::warn;

use crate::repo::node::NodeRepo;
use crate::repo::webhook::WebhookRepo;

type HmacSha256 = Hmac<Sha256>;

/// Send a **node** event to the webhooks of the node's owner.
///
/// SECURITY: only delivers to webhooks owned by `owner_id` (the owner of the
/// resource that triggered the event), so one tenant's events never fan out to
/// another tenant's endpoint (cross-tenant metadata leak).
///
/// SECURITY: a task event is refused here, whatever passes it: it must
/// go through [`dispatch_task_event`], which first asks whether its
/// receiver still sees the task's node. Refused means not sent.
pub fn dispatch(
    webhooks: Arc<dyn WebhookRepo>,
    event: &str,
    node_id: Option<NodeId>,
    triggered_by: &str,
    owner_id: &str,
) {
    if common::webhook::is_task_event(event) {
        warn!("a task event was passed to the node dispatch: not sent");
        return;
    }
    fan_out(webhooks, event, node_id, triggered_by, owner_id);
}

/// Spawn fire-and-forget tasks that POST the payload to `owner_id`'s
/// subscribed webhooks. Private: the two ways in above decide who an
/// event is for.
fn fan_out(
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
        let mut hooks = match webhooks.list_active_for_event(&event, &owner_id).await {
            Ok(h) => h,
            Err(e) => {
                warn!("webhook list_active_for_event failed: {e}");
                return;
            }
        };
        // The query reads only this owner's hooks; kept as a second
        // check that nothing else is ever delivered to.
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

/// Send a task event to the hooks of the task's author (`task_owner`),
/// if that person can still see the task's node.
///
/// SECURITY (owner decision, 2026-11-16): a task follows its node's
/// rights, not its author's (ruling of 2026-11-10). Task events used to
/// go to the author's hooks whatever their rights now, so someone whose
/// share had ended kept hearing about each change to the tasks they had
/// written there: the event, the node's id, who did it and when.
///
/// "Can see" is the rule the task lists use (`visible_to`): they own the
/// node or hold a grant on it. It is read for the author as an ordinary
/// account. An admin reads every node by their role, but the role is in
/// their token, and this is not their request: an admin's hooks hear
/// about their tasks on nodes they own or were given, and no others.
///
/// The question is asked after the response, with the event. If it can't
/// be answered, nothing is sent. A task moved onto a node its author
/// can't see is no longer told to them either.
pub fn dispatch_task_event(
    webhooks: Arc<dyn WebhookRepo>,
    nodes: Arc<dyn NodeRepo>,
    event: &str,
    node_id: NodeId,
    triggered_by: &str,
    task_owner: &str,
) {
    let (event, triggered_by, task_owner) = (
        event.to_string(),
        triggered_by.to_string(),
        task_owner.to_string(),
    );
    tokio::spawn(async move {
        let seen = nodes.visible_titles(&[node_id.0], Some(&task_owner)).await;
        if sees_node(seen, node_id) {
            fan_out(webhooks, &event, Some(node_id), &triggered_by, &task_owner);
        }
    });
}

/// Whether the read of what someone may view says they see `node`. A
/// failed read is "no": better a missed event than one sent to someone
/// who should not have it.
fn sees_node(
    seen: Result<std::collections::BTreeMap<uuid::Uuid, String>, common::EmberTroveError>,
    node: NodeId,
) -> bool {
    match seen {
        Ok(titles) => titles.contains_key(&node.0),
        Err(e) => {
            warn!("webhook: could not tell who sees a node, so nothing is sent: {e}");
            false
        }
    }
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

    use super::{in_a_slot, sees_node};

    #[test]
    fn a_task_event_is_for_someone_who_sees_the_node_and_a_failed_read_is_a_no() {
        let node = common::id::NodeId(uuid::Uuid::from_u128(7));
        let other = uuid::Uuid::from_u128(8);
        let seen = |ids: &[uuid::Uuid]| {
            Ok(ids
                .iter()
                .map(|id| (*id, "a title".to_string()))
                .collect::<std::collections::BTreeMap<_, _>>())
        };
        assert!(sees_node(seen(&[node.0]), node));
        assert!(!sees_node(seen(&[]), node), "not theirs to see");
        assert!(
            !sees_node(seen(&[other]), node),
            "another node is not this one"
        );
        let failed = Err(common::EmberTroveError::Internal("down".to_string()));
        assert!(!sees_node(failed, node), "fails closed");
    }

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
