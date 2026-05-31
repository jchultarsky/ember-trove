use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use common::{
    auth::AuthClaims,
    edge::{CreateEdgeRequest, Edge},
    id::EdgeId,
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_edges).post(create_edge))
        .route("/{id}", delete(delete_edge))
}

async fn list_edges(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<Edge>>, ApiError> {
    // SECURITY: scope to edges whose endpoints the caller owns; do NOT rely on
    // client-side filtering for an authz boundary (it leaks node UUIDs). Admins
    // (None) see all edges.
    let subject = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };
    let edges = state.edges.list_visible(subject).await?;
    Ok(Json(edges))
}

async fn create_edge(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateEdgeRequest>,
) -> Result<(StatusCode, Json<Edge>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    // Caller must have editor access on the source node.
    require_editor(state.permissions.as_ref(), &claims, req.source_id).await?;
    let edge = state.edges.create(req).await?;
    Ok((StatusCode::CREATED, Json(edge)))
}

async fn delete_edge(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let edge = state.edges.get(EdgeId(id)).await?;
    require_editor(state.permissions.as_ref(), &claims, edge.source_id).await?;
    state.edges.delete(EdgeId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
