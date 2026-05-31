use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use common::{
    auth::AuthClaims,
    graph::{NodePosition, SavePositionRequest, SavePositionsRequest},
    id::NodeId,
};
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_viewer},
    error::ApiError,
    state::AppState,
};

/// Hard cap on a single batch position write (DoS guard).
const MAX_BATCH_POSITIONS: usize = 2000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/positions", get(list_positions))
        .route("/positions", put(upsert_positions_batch))
        .route("/positions/{node_id}", put(upsert_position))
}

async fn list_positions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<NodePosition>>, ApiError> {
    // SECURITY: scope to the caller's own nodes so positions can't be used to
    // enumerate other owners' node IDs. Admins (None) see all.
    let subject = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };
    let positions = state.graph.list_positions(subject).await?;
    Ok(Json(positions))
}

async fn upsert_positions_batch(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<SavePositionsRequest>,
) -> Result<StatusCode, ApiError> {
    if req.positions.len() > MAX_BATCH_POSITIONS {
        return Err(ApiError::Validation(format!(
            "too many positions (max {MAX_BATCH_POSITIONS})"
        )));
    }
    // SECURITY: the repo write is owner-scoped (rows for nodes the caller
    // doesn't own are dropped), so a client can't tamper with other owners'
    // graph layouts. Admins (None) may write any node's position.
    let subject = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };
    let tuples: Vec<(Uuid, f64, f64)> = req
        .positions
        .into_iter()
        .map(|(node_id, x, y)| (node_id.0, x, y))
        .collect();
    state.graph.save_positions(&tuples, subject).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn upsert_position(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(req): Json<SavePositionRequest>,
) -> Result<StatusCode, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    state.graph.upsert_position(node_id, req.x, req.y).await?;
    Ok(StatusCode::NO_CONTENT)
}
