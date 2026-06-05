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

/// Delete the attachment record and the corresponding S3 object.
async fn delete_attachment(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let attachment = state.attachments.get(AttachmentId(id)).await?;
    require_editor(state.permissions.as_ref(), &claims, attachment.node_id).await?;
    let s3_key = state.attachments.delete(AttachmentId(id)).await?;
    // Best-effort S3 delete — log but don't fail the request if S3 is unavailable.
    if let Err(e) = state.object_store.delete(&s3_key).await {
        tracing::warn!(s3_key, error = %e, "S3 delete failed after DB record removed");
    }
    Ok(StatusCode::NO_CONTENT)
}
