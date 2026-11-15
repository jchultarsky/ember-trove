use common::activity::{
    ActivityAction, ActivityEntry, ActivityScope, MORE_HEADER, RecentActivityEntry,
};
use common::id::NodeId;

use super::{api_url, get_json, get_request, parse_json, send_authed};
use crate::error::UiError;

/// The entries of `rows`, without those written by a newer release: a
/// row whose `action` this build has no name for is skipped (a tab that
/// had not reloaded since a deploy used to lose the whole list to one).
/// Any other row that can't be read is an error, as before: a list that
/// quietly came back short would read as "nothing recorded".
fn known<T: serde::de::DeserializeOwned>(rows: Vec<serde_json::Value>) -> Result<Vec<T>, UiError> {
    let mut read = Vec::with_capacity(rows.len());
    for row in rows {
        let unknown_action = row
            .get("action")
            .and_then(|a| a.as_str())
            .is_some_and(|a| ActivityAction::from_db_str(a).is_none());
        match serde_json::from_value(row) {
            Ok(entry) => read.push(entry),
            Err(_) if unknown_action => {}
            Err(e) => return Err(UiError::Parse(e.to_string())),
        }
    }
    Ok(read)
}

/// A node's log: its History, its audit trail, or (no scope) both.
pub async fn fetch_activity(
    node_id: NodeId,
    limit: Option<u32>,
    scope: Option<ActivityScope>,
) -> Result<Vec<ActivityEntry>, UiError> {
    let mut query: Vec<String> = Vec::new();
    if let Some(n) = limit {
        query.push(format!("limit={n}"));
    }
    if let Some(scope) = scope {
        query.push(format!("scope={}", scope.as_str()));
    }
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };
    let rows: Vec<serde_json::Value> =
        get_json(&format!("/nodes/{node_id}/activity{query}")).await?;
    known(rows)
}

/// How many changes a node's History asks for at once: the route's most.
pub const HISTORY_CHANGES: u32 = 200;

/// The changes in a node's History (S6d), newest first, and whether the
/// node has older ones than these. The server says so in a header; the
/// count can't (it leaves out an entry this reader may not see after its
/// limit).
pub async fn fetch_history_changes(node_id: NodeId) -> Result<(Vec<ActivityEntry>, bool), UiError> {
    let url = api_url(&format!(
        "/nodes/{node_id}/activity?scope={}&limit={HISTORY_CHANGES}",
        ActivityScope::History.as_str()
    ));
    let resp = send_authed(|| get_request(&url)).await?;
    let more = resp.headers().get(MORE_HEADER).is_some();
    let rows: Vec<serde_json::Value> = parse_json(resp).await?;
    Ok((known(rows)?, more))
}

/// Recent activity recap for the home dashboard (Phase 7 / v2.9.0).
/// Returns up to `limit` entries since `since_iso` (RFC 3339).
pub async fn fetch_dashboard_activity(
    since_iso: &str,
    limit: u32,
) -> Result<Vec<RecentActivityEntry>, UiError> {
    let rows: Vec<serde_json::Value> = get_json(&format!(
        "/dashboard/activity?since={}&limit={limit}",
        js_sys::encode_uri_component(since_iso)
    ))
    .await?;
    known(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(action: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "node_id": "00000000-0000-0000-0000-000000000002",
            "subject_id": "alice",
            "action": action,
            "metadata": {},
            "created_at": "2026-11-14T10:00:00Z",
        })
    }

    #[test]
    fn an_action_this_build_has_no_name_for_is_skipped_not_fatal() {
        let read: Vec<ActivityEntry> = known(vec![
            entry("status_changed"),
            entry("something_a_newer_release_writes"),
            entry("lifecycle_changed"),
        ])
        .expect("readable");
        assert_eq!(read.len(), 2, "the unknown one is left out, the rest stay");
    }

    #[test]
    fn a_row_that_is_broken_some_other_way_is_still_an_error() {
        // A known action with a field missing: not "a newer release", so
        // not something to pass over in silence.
        let mut broken = entry("status_changed");
        if let Some(obj) = broken.as_object_mut() {
            obj.remove("created_at");
        }
        let read: Result<Vec<ActivityEntry>, _> = known(vec![entry("edited"), broken]);
        assert!(matches!(read, Err(UiError::Parse(_))));
        // Nor is a row with no action at all.
        let read: Result<Vec<ActivityEntry>, _> = known(vec![serde_json::json!({ "id": 1 })]);
        assert!(read.is_err());
    }
}
