use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::WebhookId;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Webhook {
    pub id: WebhookId,
    pub owner_id: String,
    pub url: String,
    /// Optional shared secret used to HMAC-sign payloads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    pub events: Vec<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateWebhookRequest {
    #[garde(skip)]
    pub url: String,
    #[garde(skip)]
    pub secret: Option<String>,
    #[garde(skip)]
    #[serde(default = "default_events")]
    pub events: Vec<String>,
}

// Canonical webhook event names. Delivery matches a fired event against each
// webhook's `events` array by exact string equality (`$1 = ANY(events)`), so
// handlers and clients MUST use these exact strings — hence the shared consts.
pub const EVENT_NODE_CREATED: &str = "node.created";
pub const EVENT_NODE_UPDATED: &str = "node.updated";
pub const EVENT_NODE_DELETED: &str = "node.deleted";
pub const EVENT_TASK_CREATED: &str = "task.created";
pub const EVENT_TASK_UPDATED: &str = "task.updated";
pub const EVENT_TASK_DELETED: &str = "task.deleted";

/// Every event a webhook may subscribe to (the canonical allowlist). Used for
/// the default subscription and as the documented set the UI/clients pick from.
pub fn available_events() -> &'static [&'static str] {
    &[
        EVENT_NODE_CREATED,
        EVENT_NODE_UPDATED,
        EVENT_NODE_DELETED,
        EVENT_TASK_CREATED,
        EVENT_TASK_UPDATED,
        EVENT_TASK_DELETED,
    ]
}

/// Default subscription when a create request omits `events`: all of them.
fn default_events() -> Vec<String> {
    available_events()
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_events_are_unique_and_shaped() {
        let evs = available_events();
        assert_eq!(evs.len(), 6, "expected six canonical events");
        let mut sorted = evs.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), evs.len(), "event names must be unique");
        for e in evs {
            let (cat, action) = e.split_once('.').expect("event is `category.action`");
            assert!(matches!(cat, "node" | "task"), "unexpected category: {cat}");
            assert!(!action.is_empty(), "empty action in {e}");
        }
    }

    #[test]
    fn update_request_secret_absent_null_and_value_deserialize_distinctly() {
        let base = r#""url":"https://x.example/h","events":[],"is_active":true"#;
        let absent: UpdateWebhookRequest =
            serde_json::from_str(&format!("{{{base}}}")).expect("absent");
        assert_eq!(absent.secret, None, "absent field must mean keep");
        let null: UpdateWebhookRequest =
            serde_json::from_str(&format!("{{{base},\"secret\":null}}")).expect("null");
        assert_eq!(null.secret, Some(None), "null must mean clear");
        let set: UpdateWebhookRequest =
            serde_json::from_str(&format!("{{{base},\"secret\":\"s3cret\"}}")).expect("value");
        assert_eq!(set.secret, Some(Some("s3cret".to_string())));
    }

    #[test]
    fn update_request_serializes_absent_secret_as_absent() {
        let req = UpdateWebhookRequest {
            url: "https://x.example/h".to_string(),
            secret: None,
            events: vec![],
            is_active: true,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(
            !json.contains("secret"),
            "None must serialize as an absent field (keep), got {json}"
        );
    }

    #[test]
    fn default_events_match_available() {
        let defaults = default_events();
        let avail: Vec<String> = available_events()
            .iter()
            .map(|e| (*e).to_string())
            .collect();
        assert_eq!(defaults, avail, "defaults should cover the full allowlist");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateWebhookRequest {
    #[garde(skip)]
    pub url: String,
    /// PATCH semantics via `deser_double_opt` (the `UpdateTaskRequest`
    /// pattern): field **absent** → keep the stored secret unchanged;
    /// **null** → clear it; **string** → replace it. Clients only ever see
    /// the masked secret, so "echo it back" is never correct — absence is
    /// the only safe default.
    #[garde(skip)]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::task::deser_double_opt"
    )]
    #[schema(value_type = Option<String>, nullable)]
    pub secret: Option<Option<String>>,
    #[garde(skip)]
    pub events: Vec<String>,
    #[garde(skip)]
    pub is_active: bool,
}

/// Payload sent to webhook endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookPayload {
    pub event: String,
    pub webhook_id: WebhookId,
    pub node_id: Option<crate::id::NodeId>,
    pub triggered_by: String,
    pub timestamp: DateTime<Utc>,
}
