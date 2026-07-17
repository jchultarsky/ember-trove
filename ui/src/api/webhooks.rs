use super::{delete_empty, get_json, post_json, put_json};
use crate::error::UiError;
use common::webhook::{CreateWebhookRequest, UpdateWebhookRequest, Webhook};

pub async fn list_webhooks() -> Result<Vec<Webhook>, UiError> {
    get_json("/webhooks").await
}

pub async fn create_webhook(req: &CreateWebhookRequest) -> Result<Webhook, UiError> {
    post_json("/webhooks", req).await
}

/// Note the secret PATCH semantics on `UpdateWebhookRequest`: leave `secret`
/// as `None` to keep the stored secret (the server only ever returns it
/// masked), `Some(None)` to clear it, `Some(Some(v))` to replace it.
pub async fn update_webhook(
    id: uuid::Uuid,
    req: &UpdateWebhookRequest,
) -> Result<Webhook, UiError> {
    put_json(&format!("/webhooks/{id}"), req).await
}

pub async fn delete_webhook(id: uuid::Uuid) -> Result<(), UiError> {
    delete_empty(&format!("/webhooks/{id}")).await
}
