use common::id::NodeId;

use super::{get_json, post_action_json};
use crate::error::UiError;

pub async fn fetch_versions(
    node_id: NodeId,
    limit: Option<u32>,
) -> Result<Vec<common::node_version::NodeVersion>, UiError> {
    match limit {
        Some(n) => get_json(&format!("/nodes/{node_id}/versions?limit={n}")).await,
        None => get_json(&format!("/nodes/{node_id}/versions")).await,
    }
}

pub async fn restore_version(
    node_id: NodeId,
    version_id: uuid::Uuid,
) -> Result<common::node::Node, UiError> {
    post_action_json(&format!("/nodes/{node_id}/versions/{version_id}/restore")).await
}
