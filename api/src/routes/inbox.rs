//! Quick-capture endpoint — `POST /api/inbox/quick`.
//!
//! Lands a single low-friction Task in the caller's Inbox (`tasks` row with
//! `node_id IS NULL`).  Used by the iOS Web Share Target SW handler and the
//! in-app fast-capture textarea.  See `common::inbox` for the DTOs and the
//! coalesce/truncate rules.

use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::post,
};
use common::{
    auth::AuthClaims,
    inbox::{coalesce_capture, QuickCaptureRequest, QuickCaptureResponse},
    task::{CreateTaskRequest, TaskPriority, TaskStatus},
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

    let (title, truncated) = coalesce_capture(req.title.as_deref(), req.body.as_deref());
    if title.is_empty() {
        return Err(ApiError::Validation(
            "quick capture requires non-empty title or body".to_string(),
        ));
    }

    let create_req = CreateTaskRequest {
        title,
        node_id: None,
        status: Some(TaskStatus::Open),
        priority: Some(TaskPriority::Medium),
        focus_date: None,
        due_date: None,
        recurrence: None,
    };

    let task = state.tasks.create(None, &claims.sub, create_req).await?;

    Ok((
        StatusCode::CREATED,
        Json(QuickCaptureResponse {
            id: task.id,
            truncated,
        }),
    ))
}
