use common::{
    edge::{
        ConvertEdgeRequest, CreateEdgeRequest, Edge, EdgeType, EdgeWithTitles, NodeHome,
        SetHomeRequest,
    },
    id::{EdgeId, NodeId},
};

use super::{delete_empty, get_json, post_empty, post_json, put_json};
use crate::error::UiError;

pub async fn fetch_all_edges() -> Result<Vec<Edge>, UiError> {
    get_json("/edges").await
}

pub async fn fetch_edges_for_node(node_id: NodeId) -> Result<Vec<EdgeWithTitles>, UiError> {
    get_json(&format!("/nodes/{node_id}/edges")).await
}

pub async fn fetch_backlinks(node_id: NodeId) -> Result<Vec<common::node::Node>, UiError> {
    get_json(&format!("/nodes/{node_id}/backlinks")).await
}

pub async fn create_edge(req: &CreateEdgeRequest) -> Result<Edge, UiError> {
    post_json("/edges", req).await
}

pub async fn delete_edge(id: EdgeId) -> Result<(), UiError> {
    delete_empty(&format!("/edges/{id}")).await
}

/// `POST /edges/{id}/convert` (#228): the link becomes `to`, in one step.
pub async fn convert_edge(id: uuid::Uuid, to: EdgeType) -> Result<(), UiError> {
    post_empty(
        &format!("/edges/{id}/convert"),
        &ConvertEdgeRequest { edge_type: to },
    )
    .await
}

/// `GET /nodes/{id}/home` (#228): the node's one home, as far as the
/// caller may know it.
pub async fn fetch_home(node_id: NodeId) -> Result<NodeHome, UiError> {
    get_json(&format!("/nodes/{node_id}/home")).await
}

/// `PUT /nodes/{id}/home` (#228): a new home, or `None` to stand alone.
pub async fn set_home(node_id: NodeId, parent_id: Option<NodeId>) -> Result<NodeHome, UiError> {
    put_json(
        &format!("/nodes/{node_id}/home"),
        &SetHomeRequest { parent_id },
    )
    .await
}
