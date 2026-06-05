//! Admin routes — user management via the Cognito Identity Provider Admin API.
//!
//! All endpoints require a valid JWT **and** the `admin` group membership.

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use common::{
    admin::{AdminUser, CreateAdminUserRequest, UpdateUserRolesRequest},
    auth::AuthClaims,
};
use garde::Validate;

use crate::{
    admin::CognitoAdminClient, auth::permissions::require_admin, error::ApiError, state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/roles", get(list_roles))
        .route("/users/{id}", delete(delete_user))
        .route("/users/{id}/roles", put(set_user_roles))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Unwrap the Cognito admin client or return 503.
fn cognito_client(state: &AppState) -> Result<Arc<CognitoAdminClient>, ApiError> {
    state.cognito_admin.clone().ok_or_else(|| {
        ApiError::Internal("Cognito admin is not configured (set COGNITO_USER_POOL_ID)".to_string())
    })
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /admin/users` — list all pool users with their groups.
async fn list_users(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<AdminUser>>, ApiError> {
    require_admin(&claims)?;
    let users = cognito_client(&state)?.list_users().await?;
    Ok(Json(users))
}

/// `POST /admin/users` — create a Cognito user, optionally assign groups and send welcome email.
async fn create_user(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateAdminUserRequest>,
) -> Result<(StatusCode, Json<AdminUser>), ApiError> {
    require_admin(&claims)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let user = cognito_client(&state)?.create_user(&req).await?;
    Ok((StatusCode::CREATED, Json(user)))
}

/// `DELETE /admin/users/{id}` — hard-delete a Cognito user by their username (UUID).
async fn delete_user(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_admin(&claims)?;
    cognito_client(&state)?.delete_user(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /admin/users/roles` — list all available groups (roles).
async fn list_roles(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<String>>, ApiError> {
    require_admin(&claims)?;
    let groups = cognito_client(&state)?.list_groups().await?;
    Ok(Json(groups))
}

/// `PUT /admin/users/{id}/roles` — replace a user's full group membership.
async fn set_user_roles(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserRolesRequest>,
) -> Result<StatusCode, ApiError> {
    require_admin(&claims)?;
    cognito_client(&state)?
        .set_user_groups(&id, &req.roles)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
