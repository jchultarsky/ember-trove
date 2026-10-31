use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use common::{
    auth::AuthClaims,
    edge::{CreateEdgeRequest, Edge, EdgeType},
    id::EdgeId,
    ontology::edge_kind_violation,
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor, require_viewer},
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
    // Caller must have editor access on the source node AND be able to see
    // the target — otherwise an edge (and `neighbors`) would reach into a
    // node they have no access to. `contains` claims the child's one home,
    // so it needs edit rights on the child too (§14, #224).
    require_editor(state.permissions.as_ref(), &claims, req.source_id).await?;
    if req.edge_type == EdgeType::Contains {
        require_editor(state.permissions.as_ref(), &claims, req.target_id).await?;
    } else {
        require_viewer(state.permissions.as_ref(), &claims, req.target_id).await?;
    }
    // The ontology's rules (#224) only after authZ, so a 422 can't tell a
    // stranger a node's kind. A missing node is a 404 here (an admin passes
    // authZ without a permission row).
    let source = state.nodes.get(req.source_id).await?;
    let target = state.nodes.get(req.target_id).await?;
    if let Some(refusal) = edge_kind_violation(&req.edge_type, &source.node_type, &target.node_type)
    {
        return Err(ApiError::Validation(refusal.message()));
    }
    // A second home and a loop are checked by the repo, under a lock.
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
