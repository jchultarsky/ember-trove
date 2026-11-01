use common::inbox::{QuickCaptureRequest, QuickCaptureResponse};

use super::post_json;
use crate::error::UiError;

/// `POST /api/inbox/quick` — drop one capture into the Inbox (#234).
///
/// The same contract as the service-worker share-target handler in
/// `ui/public/sw.js`, so an in-app capture and a Share Sheet capture land
/// in the same place.
pub async fn quick_capture(text: &str) -> Result<QuickCaptureResponse, UiError> {
    let req = QuickCaptureRequest {
        title: None,
        body: Some(text.to_owned()),
        due_date: None,
        priority: None,
    };
    post_json("/inbox/quick", &req).await
}
