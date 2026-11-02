use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::edge::Edge;
use crate::id::{NodeId, TagId};
use crate::node::{NodeStatus, NodeType};

/// Everything the graph view draws, in one response (`GET /api/graph`,
/// performance P1). It replaced paging through `GET /nodes`, whose rows
/// carry every body: 15.7 MB at ten times today's data, for a view that
/// shows a title and a 100-character preview.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<Edge>,
    pub positions: Vec<NodePosition>,
}

/// A node as the graph shows it: no body, a short plain-text preview.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct GraphNode {
    pub id: NodeId,
    pub title: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
    pub pinned: bool,
    pub tags: Vec<GraphTag>,
    /// `markdown::plain_preview` of the body, at most `GRAPH_PREVIEW_CHARS`.
    pub preview: Option<String>,
}

/// A tag dot: the graph filters by id and draws the colour.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct GraphTag {
    pub id: TagId,
    pub color: String,
}

/// The hover preview's length, in characters.
pub const GRAPH_PREVIEW_CHARS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodePosition {
    pub node_id: NodeId,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePositionRequest {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePositionsRequest {
    pub positions: Vec<(NodeId, f64, f64)>,
}
