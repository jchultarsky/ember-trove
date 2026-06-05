use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use common::{
    auth::AuthClaims,
    id::{NodeId, NodeLinkId},
    node_link::{CreateNodeLinkRequest, NodeLink, UpdateNodeLinkRequest},
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{require_editor, require_viewer},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_links).post(create_link))
        .route(
            "/{link_id}",
            axum::routing::put(update_link).delete(delete_link),
        )
}

async fn list_links(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<NodeLink>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let links = state.node_links.list(NodeId(node_id)).await?;
    Ok(Json(links))
}

async fn create_link(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(req): Json<CreateNodeLinkRequest>,
) -> Result<(StatusCode, Json<NodeLink>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    require_editor(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let link = state.node_links.create(NodeId(node_id), req).await?;
    Ok((StatusCode::CREATED, Json(link)))
}

async fn update_link(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((node_id, link_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateNodeLinkRequest>,
) -> Result<Json<NodeLink>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    require_editor(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let link = state
        .node_links
        .update(NodeId(node_id), NodeLinkId(link_id), req)
        .await?;
    Ok(Json(link))
}

async fn delete_link(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((node_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    state
        .node_links
        .delete(NodeId(node_id), NodeLinkId(link_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
