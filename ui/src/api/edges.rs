use common::{
    edge::{CreateEdgeRequest, Edge, EdgeWithTitles},
    id::{EdgeId, NodeId},
};

use super::{delete_empty, get_json, post_json};
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
