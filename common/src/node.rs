use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, TagId, TemplateId};
use crate::project_state::ProjectState;
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
    /// Not the lifecycle `status` above. Only a Project may have one set
    /// (a node's type never changes, §3.3); the UI edits it on Projects.
    /// Validated by `crate::project_status::normalize`.
    #[serde(default)]
    pub project_status: Option<String>,
    /// A Project's declared state (§4.1; S6a): active, parked, complete or
    /// cancelled. `Some` for every Project and `None` for every other
    /// kind (the database holds the two together). Absent from an archive
    /// older than format 10; restore then derives it from the label.
    #[serde(default)]
    pub project_state: Option<ProjectState>,
    /// When a person last declared `project_state`. `None` while nobody
    /// has: a state the migration read from the label, or a new
    /// project's "active". Never made up.
    #[serde(default)]
    pub project_state_at: Option<DateTime<Utc>>,
    pub tags: Vec<Tag>,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub edge_count: u32,
}

/// Maximum allowed markdown body length (1 MiB), on create and on update
/// alike: a body that create accepted and update refused could never be
/// saved again (#281, 1.5).
const MAX_BODY_LEN: usize = 1_048_576;

/// A title must have something to read: all spaces is as blank as empty.
/// Shared by the node and task requests.
pub(crate) fn not_blank(value: &str, _ctx: &()) -> garde::Result {
    if value.trim().is_empty() {
        return Err(garde::Error::new("must not be blank"));
    }
    Ok(())
}

/// [`not_blank`] for an optional field: absent means "leave unchanged".
pub(crate) fn not_blank_if_set(value: &Option<String>, ctx: &()) -> garde::Result {
    value.as_deref().map_or(Ok(()), |v| not_blank(v, ctx))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateNodeRequest {
    #[garde(length(min = 1, max = 500), custom(not_blank))]
    pub title: String,
    #[garde(skip)]
    pub node_type: NodeType,
    #[garde(length(max = MAX_BODY_LEN))]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, garde::Validate)]
pub struct UpdateNodeRequest {
    /// Absent leaves the title alone; a set title follows create's rules,
    /// so an update can't blank it.
    #[garde(length(min = 1, max = 500), custom(not_blank_if_set))]
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

/// The order of `GET /nodes` (#257). Pinned nodes come first in every
/// order. The server sorts, because the list is paged: sorting one page in
/// the browser ordered only the rows that happened to be loaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeSort {
    /// Most recently changed first.
    #[default]
    ModifiedDesc,
    ModifiedAsc,
    /// By title, ignoring case.
    TitleAsc,
    TitleDesc,
}

impl NodeSort {
    /// Every order, in the order the sort menu lists them.
    pub const ALL: [NodeSort; 4] = [
        NodeSort::ModifiedDesc,
        NodeSort::ModifiedAsc,
        NodeSort::TitleAsc,
        NodeSort::TitleDesc,
    ];

    /// The query-string value (`?sort=title_asc`), the same as serde's.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            NodeSort::ModifiedDesc => "modified_desc",
            NodeSort::ModifiedAsc => "modified_asc",
            NodeSort::TitleAsc => "title_asc",
            NodeSort::TitleDesc => "title_desc",
        }
    }

    /// The label in the sort menu.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            NodeSort::ModifiedDesc => "Newest first",
            NodeSort::ModifiedAsc => "Oldest first",
            NodeSort::TitleAsc => "Title A\u{2192}Z",
            NodeSort::TitleDesc => "Title Z\u{2192}A",
        }
    }
}

/// How many nodes the list shows after `more` presses of "Load more":
/// it starts at one page and doubles, so a long list is reached in a few
/// steps and few requests (#257).
#[must_use]
pub fn node_list_limit(more: u32) -> u32 {
    NODE_LIST_FIRST.saturating_mul(1u32.checked_shl(more).unwrap_or(u32::MAX))
}

/// The node list's first page.
pub const NODE_LIST_FIRST: u32 = 50;
/// The most rows one `GET /nodes` returns (`per_page` is capped to it).
pub const NODE_LIST_MAX_PER_PAGE: u32 = 200;

/// The requests that fetch the first `limit` rows: `(page, per_page)`
/// pairs with one `per_page` throughout, so the pages don't overlap.
#[must_use]
pub fn node_list_pages(limit: u32) -> Vec<(u32, u32)> {
    let per_page = limit.clamp(1, NODE_LIST_MAX_PER_PAGE);
    let pages = limit.div_ceil(per_page).max(1);
    (1..=pages).map(|page| (page, per_page)).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeListParams {
    pub node_type: Option<NodeType>,
    pub status: Option<NodeStatus>,
    /// Project status filter (#143), matched case-insensitively.
    #[serde(default)]
    pub project_status: Option<String>,
    /// Only Projects in this declared state (S6a). A node of another
    /// kind has no state and never matches.
    #[serde(default)]
    pub project_state: Option<ProjectState>,
    /// Leave out Projects whose state is terminal (complete, cancelled).
    /// Set by the dashboard, which shows active and parked projects; the
    /// filter runs in SQL so closed projects don't use up its page.
    #[serde(skip)]
    pub exclude_closed: bool,
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
    /// The order (#257). Absent: most recently changed first.
    #[serde(default)]
    pub sort: NodeSort,
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
    /// Project Briefs (#282): a Project's current Brief goes with it.
    /// Absent in an answer from before Briefs.
    #[serde(default)]
    pub briefs: u64,
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
        if self.briefs > 0 {
            out.push(count(self.briefs, "brief", "briefs"));
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
            briefs: self.briefs + other.briefs,
        }
    }
}

#[cfg(test)]
mod list_tests {
    use super::{NodeSort, node_list_limit, node_list_pages};

    #[test]
    fn the_sort_names_match_the_wire() {
        for sort in NodeSort::ALL {
            let wire = serde_json::to_value(sort).expect("json");
            assert_eq!(wire, sort.as_str(), "{sort:?}");
            assert!(!sort.label().is_empty());
        }
        assert_eq!(NodeSort::default(), NodeSort::ModifiedDesc);
    }

    #[test]
    fn load_more_doubles_from_one_page() {
        assert_eq!(node_list_limit(0), 50);
        assert_eq!(node_list_limit(1), 100);
        assert_eq!(node_list_limit(3), 400);
        // Never overflows, however often it is pressed.
        assert_eq!(node_list_limit(40), u32::MAX);
    }

    #[test]
    fn the_pages_cover_the_limit_without_overlap() {
        assert_eq!(node_list_pages(50), [(1, 50)]);
        assert_eq!(node_list_pages(100), [(1, 100)]);
        assert_eq!(node_list_pages(200), [(1, 200)]);
        assert_eq!(node_list_pages(400), [(1, 200), (2, 200)]);
        assert_eq!(node_list_pages(800).len(), 4);
        // Every step of Load more is whole pages of one size.
        for more in 0..8 {
            let limit = super::node_list_limit(more);
            let pages = node_list_pages(limit);
            let total: u32 = pages.iter().map(|(_, per)| per).sum();
            assert_eq!(total, limit, "limit {limit}");
        }
        assert_eq!(node_list_pages(0), [(1, 1)]);
    }
}

#[cfg(test)]
mod validation_tests {
    use super::{CreateNodeRequest, MAX_BODY_LEN, NodeType, UpdateNodeRequest};
    use garde::Validate;

    fn create(title: &str, body: Option<String>) -> CreateNodeRequest {
        CreateNodeRequest {
            title: title.to_string(),
            node_type: NodeType::Article,
            body,
            metadata: serde_json::json!({}),
            status: None,
            project_status: None,
            template_id: None,
        }
    }

    fn update(title: Option<&str>, body: Option<String>) -> UpdateNodeRequest {
        UpdateNodeRequest {
            title: title.map(str::to_string),
            body,
            metadata: None,
            status: None,
            project_status: None,
            expected_updated_at: None,
        }
    }

    #[test]
    fn an_update_cannot_blank_a_title() {
        // #281, 1.5: create required a title, update accepted "".
        assert!(update(Some(""), None).validate().is_err());
        assert!(update(Some("   "), None).validate().is_err());
        assert!(update(Some("Kept"), None).validate().is_ok());
        // Absent means "leave it".
        assert!(update(None, None).validate().is_ok());
        assert!(create("  ", None).validate().is_err());
    }

    #[test]
    fn create_and_update_cap_the_body_alike() {
        let at_cap = "a".repeat(MAX_BODY_LEN);
        let over = "a".repeat(MAX_BODY_LEN + 1);
        assert!(create("T", Some(at_cap.clone())).validate().is_ok());
        assert!(update(None, Some(at_cap)).validate().is_ok());
        // Create took any size, so a node could be made that no update
        // would ever accept again.
        assert!(create("T", Some(over.clone())).validate().is_err());
        assert!(update(None, Some(over)).validate().is_err());
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
            briefs: 1,
        };
        assert_eq!(
            p.losses(),
            [
                "5 tasks (2 from other people)",
                "1 note (1 from other people)",
                "3 files",
                "1 brief"
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
            briefs: 0,
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
