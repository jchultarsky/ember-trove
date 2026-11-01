//! `/captures` (ONTOLOGY §9.1, #234): the caller's Inbox captures.
//!
//! Owner-only with no admin bypass: every handler passes `claims.sub` and
//! the repo matches it, so someone else's capture is a 404 for everyone,
//! an admin included (owner decision, like People).

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use common::{
    auth::AuthClaims,
    capture::{Capture, CaptureRequest},
    id::CaptureId,
};
use garde::Validate;
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_captures).post(create_capture))
        .route("/{id}", patch(update_capture).delete(delete_capture))
        .route("/{id}/restore", post(restore_capture))
}

/// GET /captures — the caller's captures, newest first.
async fn list_captures(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<Capture>>, ApiError> {
    Ok(Json(state.captures.list(&claims.sub).await?))
}

/// POST /captures — capture some text.
async fn create_capture(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CaptureRequest>,
) -> Result<(StatusCode, Json<Capture>), ApiError> {
    let body = valid_body(req)?;
    let capture = state.captures.create(&claims.sub, &body).await?;
    Ok((StatusCode::CREATED, Json(capture)))
}

/// PATCH /captures/{id} — reword a capture (owner only).
async fn update_capture(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<CaptureRequest>,
) -> Result<Json<Capture>, ApiError> {
    let body = valid_body(req)?;
    Ok(Json(
        state
            .captures
            .update(CaptureId(id), &claims.sub, &body)
            .await?,
    ))
}

/// DELETE /captures/{id} — a deliberate delete (§9.1), undoable.
async fn delete_capture(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.captures.delete(CaptureId(id), &claims.sub).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /captures/{id}/restore — the Undo of a delete.
async fn restore_capture(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Capture>, ApiError> {
    Ok(Json(
        state.captures.restore(CaptureId(id), &claims.sub).await?,
    ))
}

/// The text, trimmed, within the limits. Whitespace alone is no capture.
fn valid_body(req: CaptureRequest) -> Result<String, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::Validation(
            "a capture needs some text".to_string(),
        ));
    }
    Ok(body.to_string())
}
