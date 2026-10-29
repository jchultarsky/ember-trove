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

wire_enum!(NodeType {
    Article => "article",
    Project => "project",
    Area => "area",
    Resource => "resource",
    Reference => "reference",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Draft,
    Published,
    Archived,
}

wire_enum!(NodeStatus {
    Draft => "draft",
    Published => "published",
    Archived => "archived",
});

impl NodeStatus {
    /// The user-facing name, shared by the editor's state picker (#196).
    pub fn label(&self) -> &'static str {
        match self {
            NodeStatus::Draft => "Draft",
            NodeStatus::Published => "Published",
            NodeStatus::Archived => "Archived",
        }
    }

    /// One line on what the state does, shown at the point of choice
    /// (#196). Worded from the implementation, not the wish: Draft and
    /// Published are labels only (no permission or sharing check reads
    /// `status`); Archived is dropped from node lists unless the caller
    /// asks for `include_archived` (`repo::node::list`), but search still
    /// returns it.
    pub fn description(&self) -> &'static str {
        match self {
            NodeStatus::Draft => {
                "Work in progress. A label only: it doesn't change who can see the node."
            }
            NodeStatus::Published => {
                "Finished and ready to use. Also a label only: sharing decides who can see it."
            }
            NodeStatus::Archived => {
                "Retired. Hidden from node lists unless you show archived; search still finds it."
            }
        }
    }
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
    /// A project's status token ("Active", "On hold"; #143), or `None`.
    /// Not the lifecycle `status` above. Kept across type changes; the UI
    /// edits it on projects. Validated by `crate::project_status::normalize`.
    #[serde(default)]
    pub project_status: Option<String>,
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
    /// Project status token (#143). For a project, absent means: take it
    /// from a `## Status` section in `body`, else "Active".
    #[garde(skip)]
    #[serde(default)]
    pub project_status: Option<String>,
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
    /// `None` = leave unchanged · `Some(None)` = clear · `Some(Some(s))` = set
    /// (validated by `crate::project_status::normalize`; blank clears).
    #[garde(skip)]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::task::deser_double_opt"
    )]
    pub project_status: Option<Option<String>>,
    /// Optimistic-concurrency precondition (#161): the node's `updated_at`
    /// as the client last saw it. When set, the update applies only if the
    /// node is still at that version; otherwise the API answers 412 and
    /// writes nothing, so a save that lands late can never overwrite a newer
    /// one. `None` keeps last-write-wins (every caller but the node editor).
    #[garde(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeListParams {
    pub node_type: Option<NodeType>,
    pub status: Option<NodeStatus>,
    /// Project status filter (#143), matched case-insensitively.
    #[serde(default)]
    pub project_status: Option<String>,
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

/// One node-picker match from `GET /nodes/lookup`: a node whose **title**
/// contains the query, ranked exact > prefix > other (see `NodeRepo::
/// lookup_titles`). Node pickers used the full-text search, which matched
/// bodies and whole words only — "Physna" listed every node *mentioning*
/// Physna alphabetically (the node titled "Physna" fell off the 8-result
/// page) and "Phy" matched nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NodeLookupHit {
    pub id: NodeId,
    pub title: String,
    pub node_type: NodeType,
}

/// Query for `GET /nodes/lookup`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NodeLookupParams {
    pub q: String,
    /// Default 8, at most 20.
    pub limit: Option<u32>,
}

/// One suggestion for the project status picker (#143): a status the caller
/// already uses on their own nodes, with how many. Variants that differ
/// only in case are one entry, spelled the most-used way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProjectStatusCount {
    pub status: String,
    pub count: u32,
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

#[cfg(test)]
mod tests {
    use super::NodeStatus;

    #[test]
    fn every_status_has_a_label_and_a_one_line_description() {
        for s in NodeStatus::ALL {
            assert!(!s.label().is_empty());
            let d = s.description();
            assert!(!d.is_empty() && !d.contains('\n'), "{d:?}");
        }
    }

    #[test]
    fn labels_are_distinct_and_match_the_wire_names() {
        let labels: Vec<_> = NodeStatus::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["Draft", "Published", "Archived"]);
        for s in NodeStatus::ALL {
            assert_eq!(s.label().to_lowercase(), s.as_str());
        }
    }

    #[test]
    fn descriptions_state_the_real_semantics() {
        // Draft/Published don't gate access; only Archived changes listing.
        assert!(NodeStatus::Draft.description().contains("label only"));
        assert!(NodeStatus::Published.description().contains("sharing"));
        assert!(
            NodeStatus::Archived
                .description()
                .contains("Hidden from node lists")
        );
        assert!(
            NodeStatus::Archived
                .description()
                .contains("search still finds it")
        );
    }
}
