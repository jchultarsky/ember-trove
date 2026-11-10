use common::{
    id::NodeId,
    node::{
        CreateNodeRequest, Node, NodeDeletionPreview, NodeListResponse, NodeTitleEntry,
        ProjectStatusCount, UpdateNodeRequest,
    },
};
use gloo_net::http::Request;

use super::{
    api_url, delete_empty, get_json, get_request, parse_json, post_action_json, post_json,
    put_json, send_authed,
};
use crate::error::UiError;

/// Fetch nodes with optional status and tag_id filters.
/// `status`: one of "draft", "published", "archived" or None for active statuses.
/// `tag_id`: UUID string of a tag to filter by, or None for all.
/// `include_archived`: when false (default for the node list), archived nodes are excluded
///   unless `status` is explicitly set to "archived" on the server side.
/// `project_status`: a project status (#143) to match, case-insensitively.
pub async fn fetch_nodes_filtered(
    status: Option<&str>,
    project_status: Option<&str>,
    tag_id: Option<uuid::Uuid>,
    include_archived: bool,
) -> Result<Vec<Node>, UiError> {
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = status {
        params.push(format!("status={}", js_sys::encode_uri_component(s)));
    }
    if let Some(ps) = project_status {
        params.push(format!(
            "project_status={}",
            js_sys::encode_uri_component(ps)
        ));
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
    let resp = send_authed(|| get_request(&url)).await?;
    let list: NodeListResponse = parse_json(resp).await?;
    Ok(list.nodes)
}

/// `GET /api/nodes/project-statuses`: the caller's own statuses, most used
/// first (the status picker's suggestions, the list filter's choices).
pub async fn fetch_project_statuses() -> Result<Vec<ProjectStatusCount>, UiError> {
    get_json("/nodes/project-statuses").await
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

/// [`update_node`] that the caller can cancel: the node editor aborts a
/// save that hangs past `common::save_watch::GIVE_UP_AFTER_MS` (#161), so a
/// stuck request can't land later over a newer retry.
pub async fn update_node_abortable(
    id: NodeId,
    req: &UpdateNodeRequest,
    signal: Option<&web_sys::AbortSignal>,
) -> Result<Node, UiError> {
    // The retry after a session renewal carries the same signal, so a
    // give-up still cancels it.
    let url = api_url(&format!("/nodes/{id}"));
    let resp = send_authed(|| {
        Request::put(&url)
            .abort_signal(signal)
            .json(req)
            .map_err(|e| UiError::Parse(e.to_string()))
    })
    .await?;
    parse_json(resp).await
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

/// Ids per `GET /nodes/deletion-preview`: under the server's cap
/// (`DELETION_PREVIEW_MAX_NODES`) and a short enough URL for any proxy.
const DELETION_PREVIEW_BATCH: usize = 50;

/// What deleting these nodes would destroy with them (#281): asked in
/// batches and summed. Nodes that aren't the caller's to delete are left
/// out by the server (`NodeDeletionPreview::kept`).
pub async fn node_deletion_preview(ids: &[NodeId]) -> Result<NodeDeletionPreview, UiError> {
    let mut total = NodeDeletionPreview::default();
    for batch in ids.chunks(DELETION_PREVIEW_BATCH) {
        let list = batch
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let part: NodeDeletionPreview =
            get_json(&format!("/nodes/deletion-preview?ids={list}")).await?;
        total = total.plus(part);
    }
    Ok(total)
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
