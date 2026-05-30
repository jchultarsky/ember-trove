use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use common::{
    auth::AuthClaims,
    id::{NodeId, NoteId},
    note::{CreateNoteRequest, FeedNote, Note, UpdateNoteRequest},
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor, require_viewer},
    error::ApiError,
    state::AppState,
};

/// Mounts under `/nodes/:node_id/notes`
pub fn node_note_router() -> Router<AppState> {
    Router::new().route("/", get(list_notes).post(create_note))
}

/// Mounts under `/notes`
pub fn note_router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_standalone_note))
        .route("/feed", get(note_feed))
        .route("/{note_id}", patch(update_note))
}

// ── Handlers ───────────────────────────────────────────────────────────────────

/// GET /nodes/:id/notes — list all notes for a node, newest first.
async fn list_notes(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<Note>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let notes = state.notes.list_for_node(NodeId(node_id)).await?;
    Ok(Json(notes))
}

/// POST /nodes/:id/notes — create a note.
async fn create_note(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(req): Json<CreateNoteRequest>,
) -> Result<(StatusCode, Json<Note>), ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let note = state.notes.create(Some(NodeId(node_id)), &claims.sub, req).await?;
    Ok((StatusCode::CREATED, Json(note)))
}

/// POST /notes — create a note, optionally attached to a node via `node_id` in
/// the body. With no `node_id` it's a standalone (inbox / micro-blog) note.
/// Attaching to a node requires editor rights on that node.
async fn create_standalone_note(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateNoteRequest>,
) -> Result<(StatusCode, Json<Note>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let node_id = req.node_id;
    if let Some(node_id) = node_id {
        require_editor(state.permissions.as_ref(), &claims, node_id).await?;
    }

    let note = state.notes.create(node_id, &claims.sub, req).await?;
    Ok((StatusCode::CREATED, Json(note)))
}

/// PATCH /notes/:note_id — edit the body of an existing note (owner only).
async fn update_note(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(note_id): Path<Uuid>,
    Json(req): Json<UpdateNoteRequest>,
) -> Result<Json<Note>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let note = state
        .notes
        .update(NoteId(note_id), &claims.sub, req)
        .await?;
    Ok(Json(note))
}

/// GET /notes/feed — notes owned by the caller, newest first, with node titles.
/// Admins see all notes.
async fn note_feed(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<FeedNote>>, ApiError> {
    let feed = if is_admin(&claims) {
        state.notes.feed_all().await?
    } else {
        state.notes.feed_for_owner(&claims.sub).await?
    };
    Ok(Json(feed))
}
