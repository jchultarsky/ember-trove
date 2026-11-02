//! Quick-capture endpoint — `POST /api/inbox/quick`.
//!
//! Lands one capture in the caller's Inbox (ONTOLOGY §9.1, #234). Used by
//! the iOS Web Share Target (via `sw.js`) and the in-app quick capture. See
//! `common::inbox` for the wire format, which installed clients still send
//! as it was when this made a task.

use axum::{Extension, Json, Router, extract::State, http::StatusCode, routing::post};
use common::{
    auth::AuthClaims,
    inbox::{QuickCaptureRequest, QuickCaptureResponse, capture_text},
};
use garde::Validate;

use crate::{error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/quick", post(quick_capture))
}

async fn quick_capture(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<QuickCaptureRequest>,
) -> Result<(StatusCode, Json<QuickCaptureResponse>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let (text, truncated) = capture_text(&req);
    if text.is_empty() {
        return Err(ApiError::Validation(
            "quick capture requires non-empty title or body".to_string(),
        ));
    }

    let capture = state.captures.create(&claims.sub, &text).await?;

    Ok((
        StatusCode::CREATED,
        Json(QuickCaptureResponse {
            id: capture.id,
            truncated,
        }),
    ))
}
