use common::id::NodeId;

use super::{delete_empty, get_json, post_json};
use crate::error::UiError;

pub async fn fetch_shared_node(token: uuid::Uuid) -> Result<common::node::Node, UiError> {
    get_json(&format!("/share/{token}")).await
}

pub async fn list_share_tokens(
    node_id: NodeId,
) -> Result<Vec<common::share_token::ShareToken>, UiError> {
    get_json(&format!("/nodes/{node_id}/share")).await
}

pub async fn create_share_token(
    node_id: NodeId,
    req: &common::share_token::CreateShareTokenRequest,
) -> Result<common::share_token::ShareToken, UiError> {
    post_json(&format!("/nodes/{node_id}/share"), req).await
}

pub async fn revoke_share_token(
    node_id: NodeId,
    token_id: common::id::ShareTokenId,
) -> Result<(), UiError> {
    delete_empty(&format!("/nodes/{node_id}/share/{token_id}")).await
}
