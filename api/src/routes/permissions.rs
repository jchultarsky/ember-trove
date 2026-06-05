/// Standalone permission routes.
///
/// These mirror the nested `nodes/{id}/permissions` routes but operate directly
/// on permission IDs, making it easy to list or update grants across nodes.
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, put},
};
use common::{
    auth::AuthClaims,
    id::PermissionId,
    permission::{Permission, PermissionListParams, PermissionRole, UpdatePermissionRequest},
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_owner},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_permissions))
        .route("/{id}", put(update_permission).delete(delete_permission))
}

/// `GET /permissions[?node_id=<uuid>]`
///
/// Returns permissions visible to the caller: only rows where the caller is
/// the subject or has Owner role on the associated node.  Admins see all.
async fn list_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<PermissionListParams>,
) -> Result<Json<Vec<Permission>>, ApiError> {
    let node_id = params.node_id.map(common::id::NodeId);
    let all_perms = state.permissions.list_all(node_id).await?;

    // Admins see everything.
    if is_admin(&claims) {
        return Ok(Json(all_perms));
    }

    // Non-admin: only rows where the caller is the subject, the granter,
    // or holds Owner role on the same node.
    let owned_nodes: std::collections::HashSet<common::id::NodeId> = all_perms
        .iter()
        .filter(|p| p.subject_id == claims.sub && matches!(p.role, PermissionRole::Owner))
        .map(|p| p.node_id)
        .collect();

    let filtered: Vec<Permission> = all_perms
        .into_iter()
        .filter(|p| p.subject_id == claims.sub || owned_nodes.contains(&p.node_id))
        .collect();

    Ok(Json(filtered))
}

/// `PUT /permissions/{id}`
///
/// Updates the role of an existing permission grant.
/// Only an owner of the associated node may modify permissions.
async fn update_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePermissionRequest>,
) -> Result<Json<Permission>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    // Look up the permission to find its parent node
    let perm = state
        .permissions
        .find_by_id(PermissionId(id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("permission {id} not found")))?;

    // Require owner on the associated node
    require_owner(state.permissions.as_ref(), &claims, perm.node_id).await?;

    let perm = state
        .permissions
        .update(PermissionId(id), req.role, &claims.sub)
        .await?;
    Ok(Json(perm))
}

/// `DELETE /permissions/{id}`
///
/// Convenience alias to the nested revoke route — operates on the permission
/// ID directly without needing to know the parent node.
/// Only an owner of the associated node may revoke permissions.
async fn delete_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Look up the permission to find its parent node
    let perm = state
        .permissions
        .find_by_id(PermissionId(id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("permission {id} not found")))?;

    // Require owner on the associated node
    require_owner(state.permissions.as_ref(), &claims, perm.node_id).await?;

    state.permissions.revoke(PermissionId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
