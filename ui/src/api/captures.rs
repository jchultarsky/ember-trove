//! `/captures` (#234): the caller's Inbox captures. Owner-only server-side.

use common::{
    capture::{Capture, CaptureRequest},
    id::CaptureId,
};

use super::{delete_empty, get_json, patch_json, post_json};
use crate::error::UiError;

pub async fn list_captures() -> Result<Vec<Capture>, UiError> {
    get_json("/captures").await
}

pub async fn create_capture(body: &str) -> Result<Capture, UiError> {
    post_json(
        "/captures",
        &CaptureRequest {
            body: body.to_string(),
        },
    )
    .await
}

pub async fn update_capture(id: CaptureId, body: &str) -> Result<Capture, UiError> {
    patch_json(
        &format!("/captures/{id}"),
        &CaptureRequest {
            body: body.to_string(),
        },
    )
    .await
}

pub async fn delete_capture(id: CaptureId) -> Result<(), UiError> {
    delete_empty(&format!("/captures/{id}")).await
}

pub async fn restore_capture(id: CaptureId) -> Result<Capture, UiError> {
    post_json(&format!("/captures/{id}/restore"), &serde_json::json!({})).await
}
