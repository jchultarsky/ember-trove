//! The data-health report (#208): existing data that breaks the data model
//! (`docs/ONTOLOGY.md`), per rule.
//!
//! The report is read-only and computed on read; nothing is stored. Each
//! account sees only its own data, naming only nodes it may read (§14). An
//! admin sees counts per rule across the installation, never titles, ids
//! or content.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// At most this many items per rule are listed; the count stays exact.
pub const ITEM_CAP: usize = 200;

/// At most this many related things per item (a loop can be huge); the
/// rest are counted in [`Violation::more`].
pub const RELATED_CAP: usize = 20;

/// One rule of the data model that existing data can break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleId {
    TaskOnNonProject,
    OrphanTask,
    MembershipOnNonProject,
    StatusOnNonProject,
    MultipleHomes,
    ContainedReference,
    ContainmentNotAllowed,
    DependsOnArea,
    RelatedToBothWays,
    ContainsCycle,
    DependsOnCycle,
    DerivedFromCycle,
}

wire_enum!(RuleId {
    TaskOnNonProject => "task_on_non_project",
    OrphanTask => "orphan_task",
    MembershipOnNonProject => "membership_on_non_project",
    StatusOnNonProject => "status_on_non_project",
    MultipleHomes => "multiple_homes",
    ContainedReference => "contained_reference",
    ContainmentNotAllowed => "containment_not_allowed",
    DependsOnArea => "depends_on_area",
    RelatedToBothWays => "related_to_both_ways",
    ContainsCycle => "contains_cycle",
    DependsOnCycle => "depends_on_cycle",
    DerivedFromCycle => "derived_from_cycle",
});

impl RuleId {
    /// The section of `docs/ONTOLOGY.md` that states the rule.
    #[must_use]
    pub fn section(self) -> &'static str {
        match self {
            Self::TaskOnNonProject | Self::StatusOnNonProject => "§3.2",
            Self::OrphanTask => "§9.3",
            Self::MembershipOnNonProject => "§7.2",
            Self::MultipleHomes | Self::ContainmentNotAllowed => "§3.3",
            Self::ContainedReference | Self::ContainsCycle => "§6.1",
            Self::RelatedToBothWays => "§6.2",
            Self::DependsOnArea | Self::DependsOnCycle => "§6.4",
            Self::DerivedFromCycle => "§6.5",
        }
    }

    /// A short heading for the rule.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::TaskOnNonProject => "Tasks outside a Project",
            Self::OrphanTask => "Tasks with no Project",
            Self::MembershipOnNonProject => "People outside a Project",
            Self::StatusOnNonProject => "A project status outside a Project",
            Self::MultipleHomes => "More than one home",
            Self::ContainedReference => "A Reference inside a home",
            Self::ContainmentNotAllowed => "A home that isn't allowed",
            Self::DependsOnArea => "A dependency on an Area",
            Self::RelatedToBothWays => "A link stored both ways",
            Self::ContainsCycle => "Homes in a loop",
            Self::DependsOnCycle => "Dependencies in a loop",
            Self::DerivedFromCycle => "Derivations in a loop",
        }
    }

    /// The rule, in one sentence.
    #[must_use]
    pub fn explanation(self) -> &'static str {
        match self {
            Self::TaskOnNonProject => "Only Projects have tasks.",
            Self::OrphanTask => "A task belongs to exactly one Project.",
            Self::MembershipOnNonProject => "Only Projects have people.",
            Self::StatusOnNonProject => "Only Projects have a project status.",
            Self::MultipleHomes => "A node has at most one home (one `contains` parent).",
            Self::ContainedReference => {
                "A Reference is never contained: it's shared, and reached by links."
            }
            Self::ContainmentNotAllowed => {
                "An Area holds Areas, Projects and Articles; a Project holds only sub-Projects."
            }
            Self::DependsOnArea => "An Area never completes, so nothing can depend on it.",
            Self::RelatedToBothWays => {
                "`related_to` has no direction, so one pair needs it only once."
            }
            Self::ContainsCycle => "Homes can't loop: no node may contain itself, even indirectly.",
            Self::DependsOnCycle => "A dependency loop is a deadlock: nothing in it could finish.",
            Self::DerivedFromCycle => "An origin comes before what was derived from it.",
        }
    }

    /// What to do about it. Fix buttons arrive with the release that owns
    /// each rule; until then these say how to fix it by hand.
    #[must_use]
    pub fn fix_hint(self) -> &'static str {
        match self {
            Self::TaskOnNonProject => "Move the task to a Project.",
            Self::OrphanTask => "Assign it to a Project, or turn it into a capture.",
            Self::MembershipOnNonProject => {
                "Add the person to a related Project, then remove them here."
            }
            Self::StatusOnNonProject => "Clear the status, or keep the note in the body.",
            Self::MultipleHomes => "Keep one home; turn the others into `related_to` links.",
            Self::ContainedReference => "Turn the `contains` edge into a `references` edge.",
            Self::ContainmentNotAllowed => "Move it to an allowed home, or link it instead.",
            Self::DependsOnArea => "Depend on a Project in the Area instead.",
            Self::RelatedToBothWays => "Delete one of the two links.",
            Self::ContainsCycle | Self::DependsOnCycle | Self::DerivedFromCycle => {
                "Remove one edge of the loop."
            }
        }
    }
}

/// What a report item points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Node,
    Task,
    Person,
}

wire_enum!(RefKind {
    Node => "node",
    Task => "task",
    Person => "person",
});

/// One thing a report item names, enough to show and link it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Ref {
    pub kind: RefKind,
    pub id: Uuid,
    pub label: String,
    /// The node is archived (hidden from lists); for a task, its node is.
    #[serde(default)]
    pub archived: bool,
    /// A task's node, for the link; `None` for an orphan task and for
    /// nodes and people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<Uuid>,
}

impl Ref {
    #[must_use]
    pub fn node(id: Uuid, label: impl Into<String>, archived: bool) -> Self {
        Self {
            kind: RefKind::Node,
            id,
            label: label.into(),
            archived,
            node_id: None,
        }
    }

    /// `archived` is the task's node's: its link leads to a hidden node.
    #[must_use]
    pub fn task(id: Uuid, label: impl Into<String>, node_id: Option<Uuid>, archived: bool) -> Self {
        Self {
            kind: RefKind::Task,
            id,
            label: label.into(),
            archived,
            node_id,
        }
    }

    #[must_use]
    pub fn person(id: Uuid, label: impl Into<String>) -> Self {
        Self {
            kind: RefKind::Person,
            id,
            label: label.into(),
            archived: false,
            node_id: None,
        }
    }
}

/// One piece of data that breaks a rule: the thing itself, and what else is
/// involved (the node a task is on, the other end of an edge, a loop's
/// other members).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Violation {
    pub subject: Ref,
    /// The first [`RELATED_CAP`] related things.
    #[serde(default)]
    pub related: Vec<Ref>,
    /// How many more related things there are beyond `related`.
    #[serde(default)]
    pub more: u64,
}

impl Violation {
    /// Keeps the first [`RELATED_CAP`] related things and counts the rest.
    #[must_use]
    pub fn new(subject: Ref, mut related: Vec<Ref>) -> Self {
        let more = related.len().saturating_sub(RELATED_CAP) as u64;
        related.truncate(RELATED_CAP);
        Self {
            subject,
            related,
            more,
        }
    }
}

/// One rule's findings for the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RuleReport {
    pub rule: RuleId,
    /// How many items break the rule, exactly.
    pub count: u64,
    /// The first [`ITEM_CAP`] of them.
    pub items: Vec<Violation>,
}

impl RuleReport {
    /// Counts every item and keeps the first [`ITEM_CAP`]: for rules
    /// computed in Rust, which hold every item anyway.
    #[must_use]
    pub fn new(rule: RuleId, mut items: Vec<Violation>) -> Self {
        let count = items.len() as u64;
        items.truncate(ITEM_CAP);
        Self { rule, count, items }
    }

    /// An exact `count` (a SQL `COUNT(*)`) with the items fetched under a
    /// `LIMIT` of [`ITEM_CAP`], so a large rule never loads every row.
    #[must_use]
    pub fn with_count(rule: RuleId, count: u64, mut items: Vec<Violation>) -> Self {
        items.truncate(ITEM_CAP);
        Self { rule, count, items }
    }
}

/// The caller's report: one entry per rule, in [`RuleId::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DataHealthReport {
    pub rules: Vec<RuleReport>,
}

/// One rule's count across the installation, for the admin view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RuleCount {
    pub rule: RuleId,
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: u128) -> Violation {
        Violation::new(
            Ref::node(Uuid::from_u128(n), format!("n{n}"), false),
            vec![],
        )
    }

    #[test]
    fn a_huge_loop_lists_a_few_members_and_counts_the_rest() {
        let members: Vec<Ref> = (0..(RELATED_CAP as u128 + 5))
            .map(|n| Ref::node(Uuid::from_u128(n), "m", false))
            .collect();
        let v = Violation::new(Ref::node(Uuid::nil(), "s", false), members);
        assert_eq!(v.related.len(), RELATED_CAP);
        assert_eq!(v.more, 5);
        let small = Violation::new(Ref::node(Uuid::nil(), "s", false), vec![]);
        assert_eq!(small.more, 0);
    }

    #[test]
    fn a_counted_report_keeps_the_sql_count_and_caps_items() {
        let items: Vec<Violation> = (0..(ITEM_CAP as u128 + 1)).map(item).collect();
        let r = RuleReport::with_count(RuleId::OrphanTask, 9_999, items);
        assert_eq!(r.count, 9_999);
        assert_eq!(r.items.len(), ITEM_CAP);
    }

    #[test]
    fn every_rule_is_explained_with_its_section_and_a_hint() {
        for rule in RuleId::ALL {
            assert!(!rule.title().is_empty(), "{}", rule.as_str());
            assert!(rule.section().starts_with('§'), "{}", rule.as_str());
            assert!(rule.explanation().ends_with('.'), "{}", rule.as_str());
            assert!(rule.fix_hint().ends_with('.'), "{}", rule.as_str());
        }
    }

    #[test]
    fn a_long_report_keeps_the_exact_count_and_the_first_items() {
        let items: Vec<Violation> = (0..(ITEM_CAP as u128 + 37)).map(item).collect();
        let report = RuleReport::new(RuleId::OrphanTask, items);
        assert_eq!(report.count, ITEM_CAP as u64 + 37);
        assert_eq!(report.items.len(), ITEM_CAP);
        assert_eq!(report.items[0], item(0), "the first ones, in order");

        let short = RuleReport::new(RuleId::OrphanTask, vec![item(1)]);
        assert_eq!((short.count, short.items.len()), (1, 1));
    }

    #[test]
    fn a_ref_round_trips_and_omits_an_empty_node_link() {
        let task = Ref::task(Uuid::from_u128(1), "t", Some(Uuid::from_u128(2)), true);
        let json = serde_json::to_value(&task).expect("ser");
        assert_eq!(json["kind"], "task");
        assert_eq!(json["archived"], true, "the task's node is archived");
        assert_eq!(serde_json::from_value::<Ref>(json).expect("de"), task);
        let node = serde_json::to_value(Ref::node(Uuid::from_u128(3), "n", true)).expect("ser");
        assert!(node.get("node_id").is_none());
        assert_eq!(node["archived"], true);
    }
}
