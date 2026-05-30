use super::{delete_empty, get_json, post_json, put_action_json, put_json};
use crate::error::UiError;

pub async fn list_templates() -> Result<Vec<common::template::NodeTemplate>, UiError> {
    get_json("/templates").await
}

pub async fn create_template(
    req: &common::template::CreateTemplateRequest,
) -> Result<common::template::NodeTemplate, UiError> {
    post_json("/templates", req).await
}

pub async fn update_template(
    id: uuid::Uuid,
    req: &common::template::UpdateTemplateRequest,
) -> Result<common::template::NodeTemplate, UiError> {
    put_json(&format!("/templates/{id}"), req).await
}

pub async fn delete_template(id: uuid::Uuid) -> Result<(), UiError> {
    delete_empty(&format!("/templates/{id}")).await
}

/// Toggle the `is_default` flag for the given template.
///
/// Returns the updated `NodeTemplate` (with `is_default` reflecting the new
/// state).  Only the template's creator may call this successfully.
pub async fn set_template_default(id: uuid::Uuid) -> Result<common::template::NodeTemplate, UiError> {
    put_action_json(&format!("/templates/{id}/set-default")).await
}
