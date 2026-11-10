//! The Project Brief (#282): `/api/nodes/{node_id}/brief`.
//!
//! SECURITY:
//! - A Brief is part of the shared node. Reading needs viewer rights on the
//!   node; saving or removing it needs editor rights. Rights are checked
//!   before anything else that touches the node, so a stranger can't tell
//!   from a 422 or a 412 what kind of node it is or whether it has a Brief.
//! - Only a Project may be given a Brief (§3.2), checked after authZ.
//! - Saves carry the version they started from; a lost race is a 412,
//!   never a silent overwrite of someone else's words.
//! - Brief text is user content: never logged. Markdown in it is rendered
//!   by the UI's ammonia pipeline.

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use chrono::{DateTime, Utc};
use common::{
    auth::AuthClaims,
    brief::{BriefInput, ProjectBrief, normalize},
    id::NodeId,
    ontology::Capability,
};
use uuid::Uuid;

use crate::{
    auth::permissions::{require_editor, require_viewer},
    error::ApiError,
    repo::activity::ActivityActor,
    routes::capability::require_capability,
    state::AppState,
};

/// `/api/nodes/{node_id}/brief`
pub fn node_router() -> Router<AppState> {
    Router::new().route("/", get(get_brief).put(save_brief).delete(delete_brief))
}

/// The node's current Brief, or `null` when it has none.
async fn get_brief(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Option<ProjectBrief>>, ApiError> {
    let node = NodeId(node_id);
    require_viewer(state.permissions.as_ref(), &claims, node).await?;
    Ok(Json(state.briefs.get(node).await?))
}

/// Save the Brief. Within the coalesce window the author's own Brief is
/// updated in place; otherwise the old one is superseded into History
/// (owner decision 2 on #282, `common::brief::save_mode`).
async fn save_brief(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(input): Json<BriefInput>,
) -> Result<Json<ProjectBrief>, ApiError> {
    let node = NodeId(node_id);
    require_editor(state.permissions.as_ref(), &claims, node).await?;
    require_capability(&state, node, Capability::Brief).await?;
    let fields = normalize(&input).map_err(|e| ApiError::Validation(e.to_string()))?;
    let (brief, _) = state
        .briefs
        .save(
            node,
            &fields,
            input.expected_updated_at,
            &ActivityActor::from_claims(&claims),
        )
        .await?;
    Ok(Json(brief))
}

#[derive(Debug, serde::Deserialize)]
struct DeleteParams {
    /// The version being removed. Required once there is a Brief: a
    /// missing or older one is a 412, as for a save.
    expected_updated_at: Option<DateTime<Utc>>,
}

/// Remove the Brief. Its words stay in History. Allowed on any kind, so a
/// Brief that predates a kind change can still be cleared.
async fn delete_brief(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Query(params): Query<DeleteParams>,
) -> Result<StatusCode, ApiError> {
    let node = NodeId(node_id);
    require_editor(state.permissions.as_ref(), &claims, node).await?;
    state
        .briefs
        .delete(
            node,
            params.expected_updated_at,
            &ActivityActor::from_claims(&claims),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
