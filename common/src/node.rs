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
    // `Resource` was here. It merged into `Reference` (ONTOLOGY §3.1,
    // migration 040, #222); see `NodeType::parse_compat`.
    Reference,
}

wire_enum!(NodeType {
    Article => "article",
    Project => "project",
    Area => "area",
    Reference => "reference",
});

impl NodeType {
    /// [`NodeType::parse`], also reading the retired `"resource"` as
    /// [`NodeType::Reference`] (#222). Only for data the browser kept from
    /// before the merge: a saved create draft, an old `?type=` link. The
    /// API stays strict and refuses `resource`.
    #[must_use]
    pub fn parse_compat(s: &str) -> Option<NodeType> {
        match s {
            "resource" => Some(NodeType::Reference),
            other => NodeType::parse(other),
        }
    }
}

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
    /// Only nodes of this kind (#216): the task pickers ask for Projects,
    /// filtered server-side so a Project is never cut off by the limit.
    #[serde(default)]
    pub node_type: Option<NodeType>,
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

/// The most nodes one `GET /nodes/deletion-preview` may ask about.
pub const DELETION_PREVIEW_MAX_NODES: usize = 100;

/// What deleting nodes destroys along with them, for the confirmation
/// (owner decision, 2026-11-09, #281): a node's tasks, notes and files are
/// deleted with it, other accounts' included, so the person deleting sees
/// the numbers first. Counts only: nothing here names a task, a note or
/// who wrote it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NodeDeletionPreview {
    /// Of the nodes asked about, how many exist and are the caller's to
    /// delete. The other counts cover only these.
    pub nodes: u64,
    /// Live tasks on them (the trash is not counted).
    pub tasks: u64,
    /// Of `tasks`, those added by an account other than the caller.
    pub others_tasks: u64,
    /// Live notes on them.
    pub notes: u64,
    /// Of `notes`, those written by an account other than the caller.
    pub others_notes: u64,
    /// Attached files.
    pub files: u64,
}

impl NodeDeletionPreview {
    /// One entry per kind of content that would be lost, in words:
    /// "5 tasks (2 from other people)". Empty when nothing would be.
    #[must_use]
    pub fn losses(&self) -> Vec<String> {
        fn count(n: u64, one: &str, many: &str) -> String {
            format!("{n} {}", if n == 1 { one } else { many })
        }
        fn with_others(n: u64, others: u64, one: &str, many: &str) -> String {
            let all = count(n, one, many);
            if others == 0 {
                all
            } else {
                format!("{all} ({others} from other people)")
            }
        }
        let mut out = Vec::new();
        if self.tasks > 0 {
            out.push(with_others(self.tasks, self.others_tasks, "task", "tasks"));
        }
        if self.notes > 0 {
            out.push(with_others(self.notes, self.others_notes, "note", "notes"));
        }
        if self.files > 0 {
            out.push(count(self.files, "file", "files"));
        }
        out
    }

    /// How many of the `asked` nodes would stay: missing, or not the
    /// caller's to delete.
    #[must_use]
    pub fn kept(&self, asked: usize) -> u64 {
        (asked as u64).saturating_sub(self.nodes)
    }

    /// The sum of two previews of different nodes (the UI asks in batches).
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            nodes: self.nodes + other.nodes,
            tasks: self.tasks + other.tasks,
            others_tasks: self.others_tasks + other.others_tasks,
            notes: self.notes + other.notes,
            others_notes: self.others_notes + other.others_notes,
            files: self.files + other.files,
        }
    }
}

#[cfg(test)]
mod deletion_preview_tests {
    use super::NodeDeletionPreview;

    #[test]
    fn losses_name_each_kind_and_how_much_is_other_peoples() {
        let p = NodeDeletionPreview {
            nodes: 1,
            tasks: 5,
            others_tasks: 2,
            notes: 1,
            others_notes: 1,
            files: 3,
        };
        assert_eq!(
            p.losses(),
            [
                "5 tasks (2 from other people)",
                "1 note (1 from other people)",
                "3 files"
            ]
        );
    }

    #[test]
    fn a_kind_with_nothing_to_lose_is_left_out() {
        let p = NodeDeletionPreview {
            nodes: 1,
            tasks: 1,
            ..Default::default()
        };
        assert_eq!(p.losses(), ["1 task"]);
        assert!(NodeDeletionPreview::default().losses().is_empty());
    }

    #[test]
    fn kept_counts_the_nodes_that_are_not_the_callers_to_delete() {
        let p = NodeDeletionPreview {
            nodes: 2,
            ..Default::default()
        };
        assert_eq!(p.kept(5), 3);
        assert_eq!(p.kept(2), 0);
        // Never negative, whatever the server answered.
        assert_eq!(p.kept(1), 0);
    }

    #[test]
    fn batches_add_up() {
        let a = NodeDeletionPreview {
            nodes: 1,
            tasks: 2,
            others_tasks: 1,
            notes: 3,
            others_notes: 0,
            files: 1,
        };
        let sum = a.plus(a);
        assert_eq!(sum.nodes, 2);
        assert_eq!(
            sum.losses(),
            ["4 tasks (2 from other people)", "6 notes", "2 files"]
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_resource_reads_as_a_reference_only_through_parse_compat() {
        use super::NodeType;
        assert_eq!(NodeType::parse("resource"), None, "the API stays strict");
        assert_eq!(
            NodeType::parse_compat("resource"),
            Some(NodeType::Reference)
        );
        assert_eq!(NodeType::parse_compat("project"), Some(NodeType::Project));
        assert_eq!(NodeType::parse_compat("nonsense"), None);
        assert!(serde_json::from_str::<NodeType>("\"resource\"").is_err());
    }

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
