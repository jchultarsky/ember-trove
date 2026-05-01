use common::{
    id::NodeId,
    node::{CreateNodeRequest, Node, NodeListResponse, NodeTitleEntry, UpdateNodeRequest},
};
use gloo_net::http::Request;

use super::{api_url, parse_json};
use crate::error::UiError;

/// Fetch ALL nodes (every page) including archived. Used by the graph view,
/// which must see the full node set so edges to nodes on later pages still
/// have visible endpoints. Loops until the server reports `has_more=false`,
/// with a hard cap to avoid pathological infinite loops if pagination ever
/// reports inconsistent state.
pub async fn fetch_nodes() -> Result<Vec<Node>, UiError> {
    const PAGE_SIZE: u32 = 200;
    const MAX_PAGES: u32 = 50;
    let mut all = Vec::new();
    let mut page: u32 = 1;
    loop {
        let url = format!(
            "{}?include_archived=true&page={page}&per_page={PAGE_SIZE}",
            api_url("/nodes"),
        );
        let resp = Request::get(&url)
            .send()
            .await
            .map_err(|e| UiError::Network(e.to_string()))?;
        let list: common::node::NodeListResponse = parse_json(resp).await?;
        let returned = list.nodes.len();
        all.extend(list.nodes);
        if !list.has_more || returned == 0 || page >= MAX_PAGES {
            break;
        }
        page += 1;
    }
    Ok(all)
}

/// Fetch nodes with optional status and tag_id filters.
/// `status`: one of "draft", "published", "archived" or None for active statuses.
/// `tag_id`: UUID string of a tag to filter by, or None for all.
/// `include_archived`: when false (default for the node list), archived nodes are excluded
///   unless `status` is explicitly set to "archived" on the server side.
pub async fn fetch_nodes_filtered(
    status: Option<&str>,
    tag_id: Option<uuid::Uuid>,
    include_archived: bool,
) -> Result<Vec<Node>, UiError> {
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = status {
        params.push(format!("status={}", js_sys::encode_uri_component(s)));
    }
    if let Some(tid) = tag_id {
        params.push(format!("tag_id={tid}"));
    }
    if include_archived {
        params.push("include_archived=true".to_owned());
    }
    let url = if params.is_empty() {
        api_url("/nodes")
    } else {
        format!("{}?{}", api_url("/nodes"), params.join("&"))
    };
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    let list: NodeListResponse = parse_json(resp).await?;
    Ok(list.nodes)
}

pub async fn fetch_node(id: NodeId) -> Result<Node, UiError> {
    let resp = Request::get(&api_url(&format!("/nodes/{id}")))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

pub async fn create_node(req: &CreateNodeRequest) -> Result<Node, UiError> {
    let resp = Request::post(&api_url("/nodes"))
        .json(req)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

pub async fn update_node(id: NodeId, req: &UpdateNodeRequest) -> Result<Node, UiError> {
    let resp = Request::put(&api_url(&format!("/nodes/{id}")))
        .json(req)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// `PUT /api/nodes/:id/pin` — toggle a node's pinned flag.  Used by
/// the v2.9.0 dashboard pin button.
pub async fn set_node_pinned(id: NodeId, pinned: bool) -> Result<Node, UiError> {
    let resp = Request::put(&api_url(&format!("/nodes/{id}/pin")))
        .json(&serde_json::json!({ "pinned": pinned }))
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

pub async fn delete_node(id: NodeId) -> Result<(), UiError> {
    let resp = Request::delete(&api_url(&format!("/nodes/{id}")))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    if resp.ok() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        Err(UiError::api(status, text))
    }
}

pub async fn duplicate_node(id: NodeId) -> Result<Node, UiError> {
    let resp = Request::post(&api_url(&format!("/nodes/{id}/duplicate")))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

pub async fn fetch_node_titles() -> Result<Vec<NodeTitleEntry>, UiError> {
    let resp = Request::get(&api_url("/nodes/titles"))
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}
