use common::{
    attachment::Attachment,
    id::{AttachmentId, NodeId},
};
use gloo_net::http::Request;

use super::{api_url, delete_empty, get_json, parse_json, send_authed};
use crate::error::UiError;

pub async fn fetch_attachments(node_id: NodeId) -> Result<Vec<Attachment>, UiError> {
    get_json(&format!("/nodes/{node_id}/attachments")).await
}

/// Upload a file using multipart/form-data.
/// The `form_data` must have a `file` field containing the File object.
pub async fn upload_attachment(
    node_id: NodeId,
    form_data: web_sys::FormData,
) -> Result<Attachment, UiError> {
    use wasm_bindgen::JsValue;
    let url = api_url(&format!("/nodes/{node_id}/attachments"));
    let resp = send_authed(|| {
        Request::post(&url)
            .body(JsValue::from(form_data.clone()))
            .map_err(|e| UiError::Parse(e.to_string()))
    })
    .await?;
    parse_json(resp).await
}

pub async fn delete_attachment(id: AttachmentId) -> Result<(), UiError> {
    delete_empty(&format!("/attachments/{id}")).await
}

#[must_use]
pub fn attachment_download_url(id: AttachmentId) -> String {
    api_url(&format!("/attachments/{id}/download"))
}
