use common::id::NodeId;

use super::get_json;
use crate::error::UiError;

pub async fn fetch_activity(
    node_id: NodeId,
    limit: Option<u32>,
) -> Result<Vec<common::activity::ActivityEntry>, UiError> {
    match limit {
        Some(n) => get_json(&format!("/nodes/{node_id}/activity?limit={n}")).await,
        None => get_json(&format!("/nodes/{node_id}/activity")).await,
    }
}

/// Recent activity recap for the home dashboard (Phase 7 / v2.9.0).
/// Returns up to `limit` entries since `since_iso` (RFC 3339).
pub async fn fetch_dashboard_activity(
    since_iso: &str,
    limit: u32,
) -> Result<Vec<common::activity::RecentActivityEntry>, UiError> {
    get_json(&format!(
        "/dashboard/activity?since={}&limit={limit}",
        js_sys::encode_uri_component(since_iso)
    ))
    .await
}
