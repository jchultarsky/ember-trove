use common::inbox::{QuickCaptureRequest, QuickCaptureResponse};

use super::post_json;
use crate::error::UiError;

/// `POST /api/inbox/quick` — drop a single low-friction task into the Inbox.
///
/// Mirrors the contract used by the service-worker share-target handler in
/// `ui/public/sw.js` so an in-app capture and a Share Sheet capture both
/// land in the same place.
pub async fn quick_capture(
    title: &str,
    body: Option<&str>,
) -> Result<QuickCaptureResponse, UiError> {
    let req = QuickCaptureRequest {
        title: Some(title.to_string()),
        body: body.map(str::to_owned),
    };
    post_json("/inbox/quick", &req).await
}
