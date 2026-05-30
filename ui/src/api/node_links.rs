use common::{
    id::{NodeId, NodeLinkId},
    node_link::{CreateNodeLinkRequest, NodeLink, UpdateNodeLinkRequest},
};

use super::{delete_empty, get_json, post_json, put_json};
use crate::error::UiError;

pub async fn fetch_node_links(node_id: NodeId) -> Result<Vec<NodeLink>, UiError> {
    get_json(&format!("/nodes/{}/links", node_id)).await
}

pub async fn create_node_link(
    node_id: NodeId,
    req: &CreateNodeLinkRequest,
) -> Result<NodeLink, UiError> {
    post_json(&format!("/nodes/{}/links", node_id), req).await
}

pub async fn update_node_link(
    node_id: NodeId,
    link_id: NodeLinkId,
    req: &UpdateNodeLinkRequest,
) -> Result<NodeLink, UiError> {
    put_json(&format!("/nodes/{}/links/{}", node_id, link_id), req).await
}

pub async fn delete_node_link(node_id: NodeId, link_id: NodeLinkId) -> Result<(), UiError> {
    delete_empty(&format!("/nodes/{}/links/{}", node_id, link_id)).await
}
