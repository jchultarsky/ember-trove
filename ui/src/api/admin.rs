use common::admin::{AdminUser, CreateAdminUserRequest, UpdateUserRolesRequest};

use super::{delete_empty, get_json, post_json, put_empty};
use crate::error::UiError;

pub async fn list_admin_users() -> Result<Vec<AdminUser>, UiError> {
    get_json("/admin/users").await
}

pub async fn create_admin_user(req: &CreateAdminUserRequest) -> Result<AdminUser, UiError> {
    post_json("/admin/users", req).await
}

pub async fn delete_admin_user(id: &str) -> Result<(), UiError> {
    delete_empty(&format!("/admin/users/{id}")).await
}

pub async fn list_realm_roles() -> Result<Vec<String>, UiError> {
    get_json("/admin/users/roles").await
}

pub async fn set_user_roles(id: &str, req: &UpdateUserRolesRequest) -> Result<(), UiError> {
    put_empty(&format!("/admin/users/{id}/roles"), req).await
}
