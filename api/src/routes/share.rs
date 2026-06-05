//! Share-token routes.
//!
//! Protected (auth required):
//!   POST   /nodes/{id}/share           — create a share token (owner only)
//!   GET    /nodes/{id}/share           — list share tokens   (owner only)
//!   DELETE /nodes/{id}/share/{token_id}— revoke a token      (owner only)
//!
//! Public (no auth):
//!   GET    /share/{token}              — read node via share token

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use common::{
    activity::ActivityAction,
    auth::AuthClaims,
    id::{NodeId, ShareTokenId},
    node::Node,
    share_token::{CreateShareTokenRequest, ShareToken},
};
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::permissions::require_owner, error::ApiError, routes::nodes::log_activity, state::AppState,
};

// ── Protected sub-router (nested under /nodes/{id}) ──────────────────────────

pub fn node_share_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_share_tokens).post(create_share_token))
        .route("/{token_id}", delete(revoke_share_token))
}

async fn create_share_token(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(req): Json<CreateShareTokenRequest>,
) -> Result<(StatusCode, Json<ShareToken>), ApiError> {
    require_owner(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let token = state
        .share_tokens
        .create(NodeId(node_id), &claims.sub, &req)
        .await?;
    log_activity(
        &state,
        NodeId(node_id),
        &claims,
        ActivityAction::Shared,
        json!({ "token_id": token.id }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(token)))
}

async fn list_share_tokens(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<ShareToken>>, ApiError> {
    require_owner(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let tokens = state.share_tokens.list(NodeId(node_id)).await?;
    Ok(Json(tokens))
}

async fn revoke_share_token(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((node_id, token_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_owner(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    state.share_tokens.revoke(ShareTokenId(token_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Public router — no auth ───────────────────────────────────────────────────

pub fn public_share_router() -> Router<AppState> {
    Router::new().route("/{token}", get(read_shared_node))
}

/// `GET /share/{token}` — returns the node if the token is valid and not expired.
/// No authentication required; the token itself is the credential.
async fn read_shared_node(
    State(state): State<AppState>,
    Path(token): Path<Uuid>,
) -> Result<Json<Node>, ApiError> {
    let share = state
        .share_tokens
        .find_by_token(token)
        .await?
        .ok_or_else(|| ApiError::NotFound("share token not found or expired".to_string()))?;

    let node = state.nodes.get(share.node_id).await?;
    Ok(Json(node))
}
