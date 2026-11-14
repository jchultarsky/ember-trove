use common::{
    id::NodeId,
    node::{
        CreateNodeRequest, Node, NodeDeletionPreview, NodeListResponse, NodeSort, NodeTitleEntry,
        ProjectStatusCount, UpdateNodeRequest,
    },
    project_state::{DeclareProjectState, ProjectState},
};
use gloo_net::http::Request;

use super::{
    api_url, delete_empty, get_json, get_request, parse_json, post_action_json, post_json,
    put_json, send_authed,
};
use crate::error::UiError;

/// The node list's filters and order, as `GET /api/nodes` takes them.
/// Everything is applied by the server, before paging (#257): the type
/// filter used to run in the browser over one page of every kind, so a
/// type's list could miss nodes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NodeListQuery {
    /// "draft", "published" or "archived"; `None` for the active ones.
    pub status: Option<String>,
    /// A project status (#143), matched case-insensitively.
    pub project_status: Option<String>,
    /// Only Projects in this declared state (S6a).
    pub project_state: Option<ProjectState>,
    pub tag_id: Option<uuid::Uuid>,
    /// Archived nodes too (they are left out by default).
    pub include_archived: bool,
    /// One kind only: "article", "project", "area", "reference".
    pub node_type: Option<String>,
    pub sort: NodeSort,
}

/// The first `limit` nodes for `query`, and how many there are in all.
/// Asked in whole pages (`common::node::node_list_pages`).
pub async fn fetch_node_list(
    query: &NodeListQuery,
    limit: u32,
) -> Result<(Vec<Node>, u64), UiError> {
    let mut filters: Vec<String> = Vec::new();
    if let Some(s) = &query.status {
        filters.push(format!("status={}", js_sys::encode_uri_component(s)));
    }
    if let Some(ps) = &query.project_status {
        filters.push(format!(
            "project_status={}",
            js_sys::encode_uri_component(ps)
        ));
    }
    if let Some(state) = query.project_state {
        filters.push(format!("project_state={}", state.as_str()));
    }
    if let Some(tid) = query.tag_id {
        filters.push(format!("tag_id={tid}"));
    }
    if query.include_archived {
        filters.push("include_archived=true".to_owned());
    }
    if let Some(nt) = &query.node_type {
        filters.push(format!("node_type={}", js_sys::encode_uri_component(nt)));
    }
    filters.push(format!("sort={}", query.sort.as_str()));

    let mut nodes: Vec<Node> = Vec::new();
    let mut total = 0u64;
    for (page, per_page) in common::node::node_list_pages(limit) {
        let url = format!(
            "{}?{}&page={page}&per_page={per_page}",
            api_url("/nodes"),
            filters.join("&")
        );
        let resp = send_authed(|| get_request(&url)).await?;
        let list: NodeListResponse = parse_json(resp).await?;
        total = list.total;
        let more = list.has_more;
        nodes.extend(list.nodes);
        if !more {
            break;
        }
    }
    Ok((nodes, total))
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

/// `PUT /api/nodes/:id/project-state`: declare a Project's state (S6a).
/// The request names the state, so sending it twice says the same thing.
///
/// Sent without a confirmation: closing a project that has something to
/// close under it is refused (412) until the confirmation dialog sends
/// the plan's token (S6b).
pub async fn declare_project_state(id: NodeId, state: ProjectState) -> Result<Node, UiError> {
    put_json(
        &format!("/nodes/{id}/project-state"),
        &DeclareProjectState {
            state,
            confirm: None,
        },
    )
    .await
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
