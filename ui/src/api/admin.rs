use common::admin::{AdminUser, CreateAdminUserRequest, PurgeCounts, UpdateUserRolesRequest};

use super::{delete_json, get_json, post_action, post_json, put_empty};
use crate::error::UiError;

pub async fn list_admin_users() -> Result<Vec<AdminUser>, UiError> {
    get_json("/admin/users").await
}

pub async fn create_admin_user(req: &CreateAdminUserRequest) -> Result<AdminUser, UiError> {
    post_json("/admin/users", req).await
}

/// Delete the account and everything it owns (#239); answers what went.
pub async fn delete_admin_user(id: &str) -> Result<PurgeCounts, UiError> {
    delete_json(&format!("/admin/users/{id}")).await
}

/// What deleting the account would remove. Counts only.
pub async fn deletion_preview(id: &str) -> Result<PurgeCounts, UiError> {
    get_json(&format!("/admin/users/{id}/deletion-preview")).await
}

/// Stop the account at once, keeping its data (#239).
pub async fn suspend_user(id: &str) -> Result<(), UiError> {
    post_action(&format!("/admin/users/{id}/suspend")).await
}

pub async fn reactivate_user(id: &str) -> Result<(), UiError> {
    post_action(&format!("/admin/users/{id}/reactivate")).await
}

pub async fn list_realm_roles() -> Result<Vec<String>, UiError> {
    get_json("/admin/users/roles").await
}

pub async fn set_user_roles(id: &str, req: &UpdateUserRolesRequest) -> Result<(), UiError> {
    put_empty(&format!("/admin/users/{id}/roles"), req).await
}
