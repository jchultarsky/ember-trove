use super::{get_json, put_empty};
use crate::error::UiError;

pub async fn fetch_positions() -> Result<Vec<common::graph::NodePosition>, UiError> {
    get_json("/graph/positions").await
}

pub async fn save_position(node_id: uuid::Uuid, x: f64, y: f64) -> Result<(), UiError> {
    let req = common::graph::SavePositionRequest { x, y };
    put_empty(&format!("/graph/positions/{node_id}"), &req).await
}

/// Batch-save all node positions at once (used by auto-arrange).
pub async fn save_positions(
    positions: &[(common::id::NodeId, f64, f64)],
) -> Result<(), UiError> {
    let req = common::graph::SavePositionsRequest {
        positions: positions.to_vec(),
    };
    put_empty("/graph/positions", &req).await
}
