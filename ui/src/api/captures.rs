//! `/captures` (#234): the caller's Inbox captures. Owner-only server-side.

use common::{
    capture::{Capture, CaptureRequest, ConvertCaptureRequest, Converted},
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

/// Turn a capture into exactly one thing (§9.1); the server deletes it in
/// the same transaction.
pub async fn convert_capture(
    id: CaptureId,
    req: &ConvertCaptureRequest,
) -> Result<Converted, UiError> {
    post_json(&format!("/captures/{id}/convert"), req).await
}
