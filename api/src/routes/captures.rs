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
    capture::{Capture, CaptureRequest, ConvertCaptureRequest, Converted},
    edge::EdgeType,
    id::CaptureId,
    node::CreateNodeRequest,
    note::CreateNoteRequest,
    ontology::{Capability, edge_kind_violation},
    task::CreateTaskRequest,
    webhook::EVENT_TASK_CREATED,
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::require_editor,
    error::ApiError,
    repo::capture::{Destination, Made},
    routes::{capability::require_capability, nodes::node_created},
    state::AppState,
    webhook_dispatch::dispatch,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_captures).post(create_capture))
        .route("/{id}", patch(update_capture).delete(delete_capture))
        .route("/{id}/restore", post(restore_capture))
        .route("/{id}/convert", post(convert_capture))
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

/// POST /captures/{id}/convert — process a capture into exactly one thing
/// (§9.1): a node, a note on a node, or a Task in a Project.
///
/// The caller's rights on the destination are checked first, as the
/// matching create routes do, then the kinds. The repo then deletes the
/// capture and makes the destination in one transaction. Someone else's
/// capture (an admin's request included) is a 404 at that point, like a
/// missing one, so its existence never shows.
async fn convert_capture(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<ConvertCaptureRequest>,
) -> Result<(StatusCode, Json<Converted>), ApiError> {
    let into = match req.checked().map_err(ApiError::Validation)? {
        ConvertCaptureRequest::Node {
            node_type,
            title,
            body,
            home,
        } => {
            if let Some(parent) = home {
                require_editor(state.permissions.as_ref(), &claims, parent).await?;
                let parent_kind = state.nodes.get(parent).await?.node_type;
                if let Some(refusal) =
                    edge_kind_violation(&EdgeType::Contains, &parent_kind, &node_type)
                {
                    return Err(ApiError::Validation(refusal.message()));
                }
            }
            let mut req = CreateNodeRequest {
                title,
                node_type,
                body,
                metadata: serde_json::json!({}),
                status: None,
                project_status: None,
                template_id: None,
            };
            // `create_node`'s own rules, so a converted node can't hold a
            // title the editor would then refuse on every save (its limit
            // counts bytes; the shared check above counts characters).
            req.validate()
                .map_err(|e| ApiError::Validation(e.to_string()))?;
            crate::routes::nodes::initial_project_status(&mut req)?;
            Destination::Node { req, home }
        }
        ConvertCaptureRequest::Note { node_id, body } => {
            require_editor(state.permissions.as_ref(), &claims, node_id).await?;
            Destination::Note {
                node_id,
                req: CreateNoteRequest {
                    body,
                    color: "default".to_string(),
                },
            }
        }
        ConvertCaptureRequest::Task {
            node_id,
            title,
            due_date,
            priority,
        } => {
            require_editor(state.permissions.as_ref(), &claims, node_id).await?;
            require_capability(&state, node_id, Capability::Tasks).await?;
            Destination::Task {
                node_id,
                req: CreateTaskRequest {
                    title,
                    node_id: Some(node_id),
                    status: None,
                    priority,
                    focus_date: None,
                    due_date,
                    recurrence: None,
                },
            }
        }
    };
    let made = state
        .captures
        .convert(CaptureId(id), &claims.sub, into)
        .await?;
    let converted = match made {
        Made::Node(node) => {
            // The capture is gone and the node exists: a wiki-link failure
            // can't undo that, and the node's next save re-links.
            if let Err(e) = node_created(&state, &claims, &node, None).await {
                tracing::warn!("wiki links of a converted capture failed (non-fatal): {e:?}");
            }
            Converted::Node { id: node.id }
        }
        Made::Note(note) => Converted::Note {
            id: note.id,
            node_id: note.node_id,
        },
        Made::Task { project, task } => {
            dispatch(
                state.webhooks.clone(),
                EVENT_TASK_CREATED,
                Some(project),
                &claims.sub,
                &task.owner_id,
            );
            Converted::Task {
                id: task.id,
                node_id: project,
            }
        }
    };
    Ok((StatusCode::CREATED, Json(converted)))
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
