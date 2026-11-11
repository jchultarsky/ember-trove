use common::{
    id::NodeId,
    project_focus::{FocusEntry, FocusInput, FocusItem, FocusOrder, FocusWhy},
};

use super::{delete_empty, get_json, put_empty, put_json};
use crate::error::UiError;

/// The caller's Focus (#282), in their order, each entry with its card.
pub async fn fetch_focus() -> Result<Vec<FocusEntry>, UiError> {
    get_json("/focus").await
}

/// The caller's Focus entry for one project, or `None` when it isn't in
/// their Focus.
pub async fn fetch_focus_item(node_id: NodeId) -> Result<Option<FocusItem>, UiError> {
    get_json(&format!("/focus/{node_id}")).await
}

/// Put a project in the caller's Focus (`FocusInput::add`).
pub async fn put_focus(node_id: NodeId, input: &FocusInput) -> Result<FocusItem, UiError> {
    put_json(&format!("/focus/{node_id}"), input).await
}

/// Change the why-line of a project that is in the caller's Focus (`None`
/// clears it). A 404 means it isn't there any more: this never adds it.
pub async fn set_focus_why(node_id: NodeId, why_now: Option<String>) -> Result<FocusItem, UiError> {
    put_json(&format!("/focus/{node_id}/why"), &FocusWhy { why_now }).await
}

/// Set the order of the caller's Focus.
pub async fn set_focus_order(node_ids: &[NodeId]) -> Result<(), UiError> {
    put_empty(
        "/focus/order",
        &FocusOrder {
            node_ids: node_ids.to_vec(),
        },
    )
    .await
}

/// Take a project out of the caller's Focus.
pub async fn remove_focus(node_id: NodeId) -> Result<(), UiError> {
    delete_empty(&format!("/focus/{node_id}")).await
}
