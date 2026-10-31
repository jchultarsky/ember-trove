//! The §3.2 write gates (#216): only Projects carry tasks, people and a
//! project status (`docs/ONTOLOGY.md`).
//!
//! SECURITY: a gate must run only after the caller is authorized on the
//! node, so its 422 can never tell a stranger what kind a node is.
//! Existing data that breaks the rule stays readable and editable; only new
//! writes are refused (the Data health page lists the rest).

use common::{
    id::NodeId,
    node::NodeType,
    ontology::{self, Capability},
};

use crate::{error::ApiError, state::AppState};

/// `kind` may carry `capability`, else 422 with the rule's reason.
pub(crate) fn refuse_unless(kind: &NodeType, capability: Capability) -> Result<(), ApiError> {
    ontology::require(kind, capability).map_err(|reason| ApiError::Validation(reason.to_string()))
}

/// Loads `node` and gates it. Call it after authorization; a missing node
/// is a 404 (an admin, who passes authorization without a permission row,
/// would otherwise reach the insert and fail on the foreign key).
pub(crate) async fn require_capability(
    state: &AppState,
    node: NodeId,
    capability: Capability,
) -> Result<(), ApiError> {
    let node = state.nodes.get(node).await?;
    refuse_unless(&node.node_type, capability)
}
