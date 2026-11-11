use common::{
    id::NodeId,
    project_focus::{FocusEntry, FocusInput, FocusItem, FocusOrder},
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

/// Put a project in the caller's Focus (`FocusInput::add`), or change its
/// why-line (`FocusInput::why`).
pub async fn put_focus(node_id: NodeId, input: &FocusInput) -> Result<FocusItem, UiError> {
    put_json(&format!("/focus/{node_id}"), input).await
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
