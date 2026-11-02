use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use common::{
    auth::AuthClaims,
    edge::{ConvertEdgeRequest, ConvertRefusal, CreateEdgeRequest, Edge, EndRights, convert_ends},
    id::{EdgeId, NodeId},
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
        .route("/{id}/convert", post(convert_edge))
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
    if req.edge_type == common::edge::EdgeType::Contains {
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
    let on_source = require_editor(state.permissions.as_ref(), &claims, edge.source_id).await;
    if edge.edge_type.deletable_from_target() && on_source.is_err() {
        // Either end may remove these (§14). A home can be released from
        // the child's side (#224): a node has only one, so its editor must
        // be able to free it even when the home is someone else's node.
        // `related_to` has no direction at all (#226).
        require_editor(state.permissions.as_ref(), &claims, edge.target_id).await?;
    } else {
        on_source?;
    }
    state.edges.delete(EdgeId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /edges/{id}/convert — a Data health fix (#228): turn a link into a
/// `references` or `related_to` link between the same nodes, in one step.
///
/// The caller must be allowed both to delete the old link and to create
/// the new one (`common::edge::convert_ends`). Rights come first, so a
/// stranger learns nothing about the link but that it exists (as with
/// DELETE); the kind rules come after, as on create.
async fn convert_edge(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<ConvertEdgeRequest>,
) -> Result<StatusCode, ApiError> {
    let edge = state.edges.get(EdgeId(id)).await?;
    let source = end_rights(&state, &claims, edge.source_id).await?;
    let target = end_rights(&state, &claims, edge.target_id).await?;
    let swap =
        convert_ends(&edge.edge_type, &req.edge_type, source, target).map_err(|r| match r {
            ConvertRefusal::Forbidden => ApiError::Forbidden(r.message().to_string()),
            ConvertRefusal::NotConvertible => ApiError::Validation(r.message().to_string()),
        })?;
    let (from, to) = if swap {
        (edge.target_id, edge.source_id)
    } else {
        (edge.source_id, edge.target_id)
    };
    let source_node = state.nodes.get(from).await?;
    let target_node = state.nodes.get(to).await?;
    if let Some(refusal) = edge_kind_violation(
        &req.edge_type,
        &source_node.node_type,
        &target_node.node_type,
    ) {
        return Err(ApiError::Validation(refusal.message()));
    }
    state
        .edges
        .convert(EdgeId(id), edge.edge_type, req.edge_type, swap)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What the caller may do to `node`. Only a refusal (403) means "no";
/// any other failure (the database) is an error, not a quiet "no".
async fn end_rights(
    state: &AppState,
    claims: &AuthClaims,
    node: NodeId,
) -> Result<EndRights, ApiError> {
    let granted = |r: Result<(), ApiError>| match r {
        Ok(()) => Ok(true),
        Err(ApiError::Forbidden(_)) => Ok(false),
        Err(e) => Err(e),
    };
    let edit = granted(require_editor(state.permissions.as_ref(), claims, node).await)?;
    let view = edit || granted(require_viewer(state.permissions.as_ref(), claims, node).await)?;
    Ok(EndRights { edit, view })
}
