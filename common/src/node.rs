use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, TagId, TemplateId};
use crate::tag::Tag;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Article,
    Project,
    Area,
    Resource,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Draft,
    Published,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Node {
    pub id: NodeId,
    pub owner_id: String,
    pub node_type: NodeType,
    pub title: String,
    pub slug: String,
    pub body: Option<String>,
    pub metadata: serde_json::Value,
    pub status: NodeStatus,
    pub tags: Vec<Tag>,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub edge_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateNodeRequest {
    #[garde(length(min = 1, max = 500))]
    pub title: String,
    #[garde(skip)]
    pub node_type: NodeType,
    #[garde(skip)]
    pub body: Option<String>,
    #[garde(skip)]
    #[serde(default = "default_metadata")]
    pub metadata: serde_json::Value,
    #[garde(skip)]
    pub status: Option<NodeStatus>,
    /// If the node was created from a template, record the template ID here
    /// for attribution in the activity log.
    #[garde(skip)]
    #[serde(default)]
    pub template_id: Option<TemplateId>,
}

fn default_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Maximum allowed markdown body length (1 MiB).
const MAX_BODY_LEN: usize = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, garde::Validate)]
pub struct UpdateNodeRequest {
    #[garde(length(max = 500))]
    pub title: Option<String>,
    #[garde(length(max = MAX_BODY_LEN))]
    pub body: Option<String>,
    #[garde(skip)]
    pub metadata: Option<serde_json::Value>,
    #[garde(skip)]
    pub status: Option<NodeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeListParams {
    pub node_type: Option<NodeType>,
    pub status: Option<NodeStatus>,
    pub tag_id: Option<TagId>,
    pub owner_id: Option<String>,
    /// When set, restricts results to nodes the subject owns or has an explicit
    /// permission row for.  Set by the API layer from the caller's JWT sub claim.
    #[serde(skip)]
    pub subject_id: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    /// When false (the default) and no explicit `status` filter is set, archived
    /// nodes are excluded from results.  Pass `include_archived=true` to opt in.
    #[serde(default)]
    pub include_archived: bool,
}

/// Lightweight title entry used for wiki-link autocomplete and resolution.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeTitleEntry {
    pub id: NodeId,
    pub title: String,
    pub slug: String,
}

/// Paginated response for node list queries.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeListResponse {
    pub nodes: Vec<Node>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub has_more: bool,
}
