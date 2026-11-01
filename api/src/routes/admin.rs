//! Admin routes — user management via the Cognito Identity Provider Admin API.
//!
//! All endpoints require a valid JWT **and** the `admin` group membership.

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use common::{
    admin::{AdminUser, CreateAdminUserRequest, UpdateUserRolesRequest},
    auth::AuthClaims,
};
use garde::Validate;

use crate::{
    admin::UserDirectory, auth::permissions::require_admin, error::ApiError, state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/roles", get(list_roles))
        .route("/users/{id}", delete(delete_user))
        .route("/users/{id}/roles", put(set_user_roles))
        .route("/users/{id}/suspend", post(suspend_user))
        .route("/users/{id}/reactivate", post(reactivate_user))
        .route("/data-health", get(data_health_counts))
}

/// `GET /api/admin/data-health` (#208): each data-health rule's count across
/// the installation. Counts only, never titles, ids or content (§14): an
/// admin sees how much cleanup is left, not anyone's data.
async fn data_health_counts(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<common::data_health::RuleCount>>, ApiError> {
    require_admin(&claims)?;
    Ok(Json(state.data_health.counts_all().await?))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Unwrap the Cognito admin client, or fail when it isn't configured.
fn cognito_client(state: &AppState) -> Result<Arc<dyn UserDirectory>, ApiError> {
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
    let mut users = cognito_client(&state)?.list_users().await?;
    // From the table, not the in-memory set: it is the record of truth.
    let suspended = state.accounts.suspended().await?;
    for user in &mut users {
        user.suspended = suspended.contains(&user.id);
    }
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
    refuse_own_account(&claims, &id, "You can't delete your own account.")?;
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
    if !req.roles.iter().any(|r| r == ADMIN_ROLE) {
        refuse_own_account(&claims, &id, "You can't remove your own admin role.")?;
    }
    cognito_client(&state)?
        .set_user_groups(&id, &req.roles)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The Cognito group that makes an account an admin.
const ADMIN_ROLE: &str = "admin";

/// What a suspend that only half-landed tells the admin. The account is
/// already refused by the API; Cognito's sign-in block didn't confirm.
const SUSPEND_UNCONFIRMED: &str = "The account is suspended in Ember Trove, but the sign-in \
     provider didn't confirm. Try again.";

/// An admin can't lock out, delete or demote their own account (#239), so
/// the last admin can't remove the only way back in.
fn refuse_own_account(claims: &AuthClaims, id: &str, message: &str) -> Result<(), ApiError> {
    if claims.sub == id {
        return Err(ApiError::Validation(message.to_string()));
    }
    Ok(())
}

/// `POST /admin/users/{id}/suspend` (#239): the account keeps its data and
/// can't sign in, and its open sessions stop at once.
///
/// Fails closed: the row and the in-memory set are written first, so the
/// API refuses the account before Cognito is asked. A Cognito failure
/// leaves it suspended here and says so; a retry is harmless. An account
/// Cognito no longer has keeps its row (an ID token it was issued may
/// still be live) and answers 404.
async fn suspend_user(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_admin(&claims)?;
    refuse_own_account(&claims, &id, "You can't suspend your own account.")?;
    let directory = cognito_client(&state)?;
    state.accounts.suspend(&id, &claims.sub).await?;
    state.suspended.insert(&id).await;
    let confirmed = async {
        directory.disable_user(&id).await?;
        directory.sign_out_everywhere(&id).await
    };
    match confirmed.await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(ApiError::NotFound(m)) => Err(ApiError::NotFound(m)),
        Err(_) => Err(ApiError::Upstream(SUSPEND_UNCONFIRMED.to_string())),
    }
}

/// `POST /admin/users/{id}/reactivate` (#239). Fails closed the other way:
/// Cognito lets the account sign in again first, and only then does the
/// API take it off the list. If Cognito fails, or no longer has the
/// account, it stays on the list (deleting the account removes the row).
async fn reactivate_user(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_admin(&claims)?;
    cognito_client(&state)?.enable_user(&id).await?;
    state.accounts.unsuspend(&id).await?;
    state.suspended.remove(&id).await;
    Ok(StatusCode::NO_CONTENT)
}
