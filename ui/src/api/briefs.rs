use chrono::{DateTime, SecondsFormat, Utc};
use common::{
    brief::{BriefInput, ProjectBrief},
    id::NodeId,
};

use super::{delete_empty, get_json, put_json};
use crate::error::UiError;

/// A project's current Brief (#282), or `None` when it has none.
pub async fn fetch_brief(node_id: NodeId) -> Result<Option<ProjectBrief>, UiError> {
    get_json(&format!("/nodes/{node_id}/brief")).await
}

/// Save the Brief. `input.expected_updated_at` is the version the editor
/// started from (`None` for a first Brief); a 412 means it has moved on.
pub async fn save_brief(node_id: NodeId, input: &BriefInput) -> Result<ProjectBrief, UiError> {
    put_json(&format!("/nodes/{node_id}/brief"), input).await
}

/// Remove the Brief at the version `expected`. Its words stay in History.
pub async fn delete_brief(node_id: NodeId, expected: DateTime<Utc>) -> Result<(), UiError> {
    // `Z`, never `+00:00`: a `+` in a query string reads as a space.
    let version = expected.to_rfc3339_opts(SecondsFormat::AutoSi, true);
    delete_empty(&format!(
        "/nodes/{node_id}/brief?expected_updated_at={version}"
    ))
    .await
}
