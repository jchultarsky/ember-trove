use common::id::NodeId;

use super::{delete_empty, get_json, post_json, put_json};
use crate::error::UiError;

/// List every permission row in the system — no node filter (admin view).
pub async fn list_all_permissions() -> Result<Vec<common::permission::Permission>, UiError> {
    get_json("/permissions").await
}

pub async fn list_permissions(
    node_id: NodeId,
) -> Result<Vec<common::permission::Permission>, UiError> {
    get_json(&format!("/nodes/{node_id}/permissions")).await
}

pub async fn grant_permission(
    node_id: NodeId,
    req: &common::permission::GrantPermissionRequest,
) -> Result<common::permission::Permission, UiError> {
    post_json(&format!("/nodes/{node_id}/permissions"), req).await
}

pub async fn revoke_permission(
    node_id: NodeId,
    perm_id: common::id::PermissionId,
) -> Result<(), UiError> {
    delete_empty(&format!("/nodes/{node_id}/permissions/{perm_id}")).await
}

pub async fn invite_to_node(
    node_id: NodeId,
    req: &common::permission::InviteRequest,
) -> Result<common::permission::Permission, UiError> {
    post_json(&format!("/nodes/{node_id}/invite"), req).await
}

pub async fn update_permission(
    perm_id: common::id::PermissionId,
    req: &common::permission::UpdatePermissionRequest,
) -> Result<common::permission::Permission, UiError> {
    put_json(&format!("/permissions/{perm_id}"), req).await
}
