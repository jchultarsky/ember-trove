use common::id::NodeId;
use gloo_net::http::Request;

use super::{api_url, parse_json};
use crate::error::UiError;

pub async fn fetch_activity(
    node_id: NodeId,
    limit: Option<u32>,
) -> Result<Vec<common::activity::ActivityEntry>, UiError> {
    let url = match limit {
        Some(n) => api_url(&format!("/nodes/{node_id}/activity?limit={n}")),
        None => api_url(&format!("/nodes/{node_id}/activity")),
    };
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// Recent activity recap for the home dashboard (Phase 7 / v2.9.0).
/// Returns up to `limit` entries since `since_iso` (RFC 3339).
pub async fn fetch_dashboard_activity(
    since_iso: &str,
    limit: u32,
) -> Result<Vec<common::activity::RecentActivityEntry>, UiError> {
    let url = api_url(&format!(
        "/dashboard/activity?since={}&limit={limit}",
        js_sys::encode_uri_component(since_iso)
    ));
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}
