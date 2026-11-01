use axum::{
    Extension, Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get},
};
use common::{auth::AuthClaims, id::AttachmentId};
use uuid::Uuid;

use crate::{
    auth::permissions::{require_editor, require_viewer},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}/download", get(download))
        .route("/{id}", delete(delete_attachment))
}

/// Stream the attachment bytes directly from S3.
async fn download(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let attachment = state.attachments.get(AttachmentId(id)).await?;
    require_viewer(state.permissions.as_ref(), &claims, attachment.node_id).await?;
    let data = state
        .object_store
        .get(&attachment.s3_key)
        .await
        .map_err(|e| ApiError::Storage(e.to_string()))?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &attachment.content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                attachment
                    .filename
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
                    .take(200)
                    .collect::<String>()
            ),
        )
        .header(header::CONTENT_LENGTH, data.len().to_string())
        // Stop the browser from MIME-sniffing a stored attachment into an
        // executable type (defense-in-depth alongside the attachment disposition).
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(data))
        .map_err(|e| ApiError::Internal(format!("response build: {e}")))
}

/// Delete the attachment record; its S3 object follows through the queue.
async fn delete_attachment(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let attachment = state.attachments.get(AttachmentId(id)).await?;
    require_editor(state.permissions.as_ref(), &claims, attachment.node_id).await?;
    // The row's delete queues its file (the attachment trigger, #239); the
    // drain deletes it from storage, and retries daily if that fails.
    state.attachments.delete(AttachmentId(id)).await?;
    state.drain_object_deletions();
    Ok(StatusCode::NO_CONTENT)
}
