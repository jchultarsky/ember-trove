use super::{get_json, put_empty};
use crate::error::UiError;

/// Everything the graph view draws, in one request (performance P1): nodes
/// without bodies, edges and saved positions.
pub async fn fetch_graph() -> Result<common::graph::GraphData, UiError> {
    get_json("/graph").await
}

pub async fn save_position(node_id: uuid::Uuid, x: f64, y: f64) -> Result<(), UiError> {
    let req = common::graph::SavePositionRequest { x, y };
    put_empty(&format!("/graph/positions/{node_id}"), &req).await
}

/// Batch-save all node positions at once (used by auto-arrange).
pub async fn save_positions(positions: &[(common::id::NodeId, f64, f64)]) -> Result<(), UiError> {
    let req = common::graph::SavePositionsRequest {
        positions: positions.to_vec(),
    };
    put_empty("/graph/positions", &req).await
}
