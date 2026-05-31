use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, SearchPresetId};
use crate::node::{NodeStatus, NodeType};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchQuery {
    pub q: String,
    pub fuzzy: Option<bool>,
    pub node_type: Option<NodeType>,
    /// When set, only nodes with this status are returned.
    pub status: Option<NodeStatus>,
    /// Comma-separated tag UUIDs. All listed tags are applied as a filter.
    pub tag_ids: Option<String>,
    /// How to combine multiple tags: `"or"` (default) or `"and"`.
    pub tag_op: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    /// Sort order: `"relevance"` (default for text search), `"updated_desc"`,
    /// `"updated_asc"`, `"title_asc"`, `"title_desc"`.
    pub sort: Option<String>,
    /// Restrict results to nodes updated on or after this date (`YYYY-MM-DD`).
    pub updated_after: Option<NaiveDate>,
    /// Restrict results to nodes updated on or before this date (`YYYY-MM-DD`).
    pub updated_before: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResult {
    pub node_id: NodeId,
    pub title: String,
    pub slug: String,
    pub snippet: Option<String>,
    pub rank: f32,
    /// Lowercase node type string, e.g. `"article"`, `"project"`.
    pub node_type: String,
    /// Lowercase status string, e.g. `"draft"`, `"published"`, `"archived"`.
    pub status: String,
    /// Where the match was found: `"node"` (title/body), `"note"`, or `"task"`.
    /// `None` for browse (empty-query) results.
    pub match_source: Option<String>,
    /// `ts_headline`-marked title when the query matched the node title.
    /// Contains `<mark>…</mark>` spans; `None` in browse mode or fuzzy mode.
    pub highlighted_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
}

// ── Search presets ─────────────────────────────────────────────────────────────

/// A saved combination of search query + filter settings.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchPreset {
    pub id: SearchPresetId,
    pub owner_id: String,
    pub name: String,
    /// The text search query (may be empty).
    pub query: String,
    pub fuzzy: bool,
    pub published_only: bool,
    /// Comma-separated tag UUID strings (mirrors `SearchQuery::tag_ids`).
    pub tag_ids: String,
    /// `"or"` (default) or `"and"`.
    pub tag_op: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, garde::Validate)]
pub struct CreateSearchPresetRequest {
    #[garde(length(min = 1, max = 100))]
    pub name: String,
    #[garde(length(max = 1000))]
    pub query: String,
    #[garde(skip)]
    pub fuzzy: bool,
    #[garde(skip)]
    pub published_only: bool,
    #[garde(length(max = 2000))]
    pub tag_ids: String,
    #[garde(length(max = 8))]
    pub tag_op: String,
}
