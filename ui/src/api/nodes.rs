use common::{
    id::NodeId,
    node::{CreateNodeRequest, Node, NodeListResponse, NodeTitleEntry, UpdateNodeRequest},
};
use gloo_net::http::Request;

use super::{api_url, delete_empty, get_json, parse_json, post_action_json, post_json, put_json};
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
    get_json(&format!("/nodes/{id}")).await
}

pub async fn create_node(req: &CreateNodeRequest) -> Result<Node, UiError> {
    post_json("/nodes", req).await
}

pub async fn update_node(id: NodeId, req: &UpdateNodeRequest) -> Result<Node, UiError> {
    put_json(&format!("/nodes/{id}"), req).await
}

/// `GET /api/nodes/:id/neighbors` — nodes this node links to (outgoing edges).
pub async fn fetch_neighbors(id: NodeId) -> Result<Vec<Node>, UiError> {
    get_json(&format!("/nodes/{id}/neighbors")).await
}

/// `PUT /api/nodes/:id/pin` — toggle a node's pinned flag.  Used by
/// the v2.9.0 dashboard pin button.
pub async fn set_node_pinned(id: NodeId, pinned: bool) -> Result<Node, UiError> {
    put_json(
        &format!("/nodes/{id}/pin"),
        &serde_json::json!({ "pinned": pinned }),
    )
    .await
}

pub async fn delete_node(id: NodeId) -> Result<(), UiError> {
    delete_empty(&format!("/nodes/{id}")).await
}

pub async fn duplicate_node(id: NodeId) -> Result<Node, UiError> {
    post_action_json(&format!("/nodes/{id}/duplicate")).await
}

/// EVERY title visible to the caller. Only for genuine full enumerations
/// (the admin bulk-permissions table); pickers use `NodePicker` and
/// rendering uses [`resolve_titles`] (#134).
pub async fn fetch_node_titles() -> Result<Vec<NodeTitleEntry>, UiError> {
    get_json("/nodes/titles").await
}

/// Resolve wiki-link titles (already lowercased) to node ids, among nodes
/// the caller can see: `{ "title": id }`, unmatched titles absent. Sent in
/// batches that stay well under the server's 100-title cap and nginx's
/// request-line limit (`GET`, so a node view never spends write budget).
pub async fn resolve_titles(
    lowered: &[String],
) -> Result<std::collections::HashMap<String, NodeId>, UiError> {
    const MAX_TITLES: usize = 50;
    const MAX_ENCODED: usize = 3_000;
    let mut out = std::collections::HashMap::new();
    let mut batch: Vec<&String> = Vec::new();
    let mut encoded_len = 0;
    let flush = |batch: &mut Vec<&String>| -> Option<String> {
        if batch.is_empty() {
            return None;
        }
        let json = serde_json::to_string(batch).ok()?;
        batch.clear();
        Some(js_sys::encode_uri_component(&json).into())
    };
    let mut queries: Vec<String> = Vec::new();
    for t in lowered {
        let len = String::from(js_sys::encode_uri_component(t)).len() + 8;
        if !batch.is_empty() && (batch.len() >= MAX_TITLES || encoded_len + len > MAX_ENCODED) {
            queries.extend(flush(&mut batch));
            encoded_len = 0;
        }
        batch.push(t);
        encoded_len += len;
    }
    queries.extend(flush(&mut batch));
    for q in queries {
        let part: std::collections::HashMap<String, NodeId> =
            get_json(&format!("/nodes/resolve-titles?titles={q}")).await?;
        out.extend(part);
    }
    Ok(out)
}
