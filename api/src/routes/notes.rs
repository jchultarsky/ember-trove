use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use chrono::{DateTime, NaiveDate, Utc};
use common::{
    auth::AuthClaims,
    id::{NodeId, NoteId},
    note::{CreateNoteRequest, FeedNote, Note, NoteFeedParams, NoteSort, UpdateNoteRequest},
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor, require_viewer},
    error::ApiError,
    repo::note::NoteFeedFilter,
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
        .route("/{note_id}", patch(update_note).delete(delete_note))
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

/// DELETE /notes/:note_id — delete a note (owner only).
async fn delete_note(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(note_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.notes.delete(NoteId(note_id), &claims.sub).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /notes/feed — notes with node titles, filtered + sorted per query params
/// (node_id, uncategorized, from, to, q, sort). Admins see all owners' notes.
async fn note_feed(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    axum::extract::Query(params): axum::extract::Query<NoteFeedParams>,
) -> Result<Json<Vec<FeedNote>>, ApiError> {
    let owner_id = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };

    let filter = NoteFeedFilter {
        node_id: params
            .node_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok())
            .map(NodeId),
        uncategorized: params.uncategorized.unwrap_or(false),
        from: params.from.as_deref().and_then(parse_date_start),
        to: params.to.as_deref().and_then(parse_date_end),
        q: params.q.filter(|s| !s.trim().is_empty()),
        sort: NoteSort::from_param(params.sort.as_deref()),
    };

    let feed = state.notes.feed(owner_id, &filter).await?;
    Ok(Json(feed))
}

/// Parse a `YYYY-MM-DD` date as the inclusive start-of-day (UTC).
fn parse_date_start(s: &str) -> Option<DateTime<Utc>> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc())
}

/// Parse a `YYYY-MM-DD` date as the exclusive end bound = start of the next day
/// (UTC), so the upper bound is inclusive of the whole given day.
fn parse_date_end(s: &str) -> Option<DateTime<Utc>> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?.succ_opt()?;
    d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_start_is_midnight_utc() {
        assert_eq!(
            parse_date_start("2026-05-30").map(|d| d.to_rfc3339()),
            Some("2026-05-30T00:00:00+00:00".to_string())
        );
    }

    #[test]
    fn date_end_is_next_day_midnight() {
        assert_eq!(
            parse_date_end("2026-05-30").map(|d| d.to_rfc3339()),
            Some("2026-05-31T00:00:00+00:00".to_string())
        );
    }

    #[test]
    fn invalid_dates_return_none() {
        assert!(parse_date_start("nonsense").is_none());
        assert!(parse_date_end("2026-13-99").is_none());
    }

    #[test]
    fn sort_param_parsing() {
        assert_eq!(NoteSort::from_param(Some("oldest")), NoteSort::Oldest);
        assert_eq!(NoteSort::from_param(Some("updated")), NoteSort::Updated);
        assert_eq!(NoteSort::from_param(Some("newest")), NoteSort::Newest);
        assert_eq!(NoteSort::from_param(None), NoteSort::Newest);
        assert_eq!(NoteSort::from_param(Some("garbage")), NoteSort::Newest);
    }
}
