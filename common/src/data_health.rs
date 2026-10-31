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
            Self::MultipleHomes => "A node has at most one home (one contains parent).",
            Self::ContainedReference => {
                "A Reference is never contained: it's shared, and reached by links."
            }
            Self::ContainmentNotAllowed => {
                "An Area holds Areas, Projects and Articles; a Project holds only sub-Projects."
            }
            Self::DependsOnArea => "An Area never completes, so nothing can depend on it.",
            Self::ContainsCycle => "Homes can't loop: no node may contain itself, even indirectly.",
            Self::DependsOnCycle => "A dependency loop is a deadlock: nothing in it could finish.",
            Self::DerivedFromCycle => "An origin comes before what was derived from it.",
        }
    }

    /// What to do about it. Most rules also have fix buttons on the page
    /// (#216, #228); this says what they do, or how to fix it by hand.
    #[must_use]
    pub fn fix_hint(self) -> &'static str {
        match self {
            Self::TaskOnNonProject => "Move the task to a Project.",
            Self::OrphanTask => "Assign it to a Project, or turn it into a capture.",
            Self::MembershipOnNonProject => {
                "Add the person to a related Project, then remove them here."
            }
            Self::StatusOnNonProject => "Clear the status, or keep the note in the body.",
            Self::MultipleHomes => "Keep one home; turn the others into related-to links.",
            Self::ContainedReference => "Turn the contains link into a references link.",
            Self::ContainmentNotAllowed => "Move it to an allowed home, or link it instead.",
            Self::DependsOnArea => "Depend on a Project in the Area instead.",
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
    /// A task that is done or cancelled: lists keep those in a collapsed
    /// "completed" section, where a link to the task itself can't land.
    #[serde(default)]
    pub closed: bool,
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
            closed: false,
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
            closed: false,
        }
    }

    /// Marks a task as done or cancelled (see [`Ref::closed`]).
    #[must_use]
    pub fn with_closed(mut self, closed: bool) -> Self {
        self.closed = closed;
        self
    }

    #[must_use]
    pub fn person(id: Uuid, label: impl Into<String>) -> Self {
        Self {
            kind: RefKind::Person,
            id,
            label: label.into(),
            archived: false,
            node_id: None,
            closed: false,
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
    /// The membership behind a person-on-a-non-Project item, which its
    /// "Remove" fix deletes (#216). `None` for every other rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership: Option<MembershipRef>,
    /// The links behind an edge item, which its fixes act on (#228): each
    /// home of a node with two, the one bad `contains` or `depends_on`, or
    /// a loop's links in order. Empty for every other rule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkRef>,
}

/// One link a fix can act on (#228). Both ends are nodes the reader can
/// see: the report loads only such links (§14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LinkRef {
    pub id: Uuid,
    pub edge_type: crate::edge::EdgeType,
    pub source: Uuid,
    pub target: Uuid,
    /// The reader owns the source, or the target. Fixes are offered by
    /// ownership, the way a membership's Remove is (#216); the API checks
    /// the real rights, shares included.
    pub source_owned: bool,
    pub target_owned: bool,
}

impl LinkRef {
    fn rights(owned: bool) -> crate::edge::EndRights {
        crate::edge::EndRights {
            edit: owned,
            view: true,
        }
    }

    /// The reader may delete the link (the rule of `DELETE /edges/{id}`).
    #[must_use]
    pub fn removable(&self) -> bool {
        self.source_owned || (self.edge_type.deletable_from_target() && self.target_owned)
    }

    /// The reader may turn the link into `to` (the rule of
    /// `POST /edges/{id}/convert`, [`crate::edge::convert_ends`]).
    #[must_use]
    pub fn convertible_to(&self, to: &crate::edge::EdgeType) -> bool {
        crate::edge::convert_ends(
            &self.edge_type,
            to,
            Self::rights(self.source_owned),
            Self::rights(self.target_owned),
        )
        .is_ok()
    }
}

/// The membership a person item stands for (#216).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MembershipRef {
    pub id: Uuid,
    /// The reader owns the node. Removing a membership needs the node's
    /// owner, so the fix is offered only then.
    pub node_owned: bool,
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
            membership: None,
            links: Vec::new(),
        }
    }

    /// Names the links behind an edge item (see [`Violation::links`]). A
    /// loop has one more link than related nodes, so the cap is one more.
    #[must_use]
    pub fn with_links(mut self, mut links: Vec<LinkRef>) -> Self {
        links.truncate(RELATED_CAP + 1);
        self.links = links;
        self
    }

    /// Names the membership behind a person item and whether the reader
    /// owns its node (see [`Violation::membership`]).
    #[must_use]
    pub fn with_membership(mut self, id: Uuid, node_owned: bool) -> Self {
        self.membership = Some(MembershipRef { id, node_owned });
        self
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

/// What a whole graph breaks, as counts per rule in [`RuleId::ALL`] order,
/// zero counts left out: the restore preview's warning (#224). It holds
/// counts only, never ids or titles. `kinds` maps each node to its kind;
/// an edge with an end missing from it is skipped.
///
/// It counts the way the report does: a node with two homes is one item,
/// and a loop is one per tangle of edges. A `related_to` stored both ways
/// is no rule break any more: restore merges it (#226).
#[must_use]
pub fn graph_rule_counts(
    kinds: &std::collections::HashMap<Uuid, crate::node::NodeType>,
    edges: &[(Uuid, Uuid, crate::edge::EdgeType)],
) -> Vec<(RuleId, usize)> {
    use crate::edge::EdgeType;
    use crate::node::NodeType;
    use crate::ontology::{ContainmentRule, containment_violation, cyclic_components};
    use std::collections::HashMap;

    let known: Vec<_> = edges
        .iter()
        .filter_map(|(s, t, e)| Some((*s, *t, e, kinds.get(s)?, kinds.get(t)?)))
        .collect();
    let of_type = |want: &EdgeType| -> Vec<(Uuid, Uuid)> {
        known
            .iter()
            .filter(|k| k.2 == want)
            .map(|k| (k.0, k.1))
            .collect()
    };
    let mut counts: HashMap<RuleId, usize> = HashMap::new();
    let mut bump = |rule: RuleId, n: usize| *counts.entry(rule).or_default() += n;

    let mut homes: HashMap<Uuid, usize> = HashMap::new();
    for (_, child, _, parent_kind, child_kind) in
        known.iter().filter(|k| *k.2 == EdgeType::Contains)
    {
        *homes.entry(*child).or_default() += 1;
        match containment_violation(parent_kind, child_kind) {
            Some(ContainmentRule::ContainedReference) => bump(RuleId::ContainedReference, 1),
            Some(ContainmentRule::NotAllowed) => bump(RuleId::ContainmentNotAllowed, 1),
            None => {}
        }
    }
    bump(
        RuleId::MultipleHomes,
        homes.values().filter(|&&n| n > 1).count(),
    );
    bump(
        RuleId::DependsOnArea,
        known
            .iter()
            .filter(|k| *k.2 == EdgeType::DependsOn && *k.4 == NodeType::Area)
            .count(),
    );
    for (edge_type, rule) in [
        (EdgeType::Contains, RuleId::ContainsCycle),
        (EdgeType::DependsOn, RuleId::DependsOnCycle),
        (EdgeType::DerivedFrom, RuleId::DerivedFromCycle),
    ] {
        bump(rule, cyclic_components(&of_type(&edge_type)).len());
    }

    RuleId::ALL
        .iter()
        .filter_map(|r| counts.get(r).filter(|&&n| n > 0).map(|&n| (*r, n)))
        .collect()
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
    fn the_page_and_the_api_refusal_say_the_same_thing() {
        // #216: a 422 and the Data health card for the same rule must not
        // drift apart.
        use crate::ontology::Capability;
        for (rule, cap) in [
            (RuleId::TaskOnNonProject, Capability::Tasks),
            (RuleId::MembershipOnNonProject, Capability::People),
            (RuleId::StatusOnNonProject, Capability::ProjectStatus),
        ] {
            let refusal = cap.refusal();
            let sentence = format!("O{}.", &refusal[1..]);
            assert_eq!(rule.explanation(), sentence, "{}", rule.as_str());
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

    #[test]
    fn a_links_fixes_follow_the_apis_rules() {
        use crate::edge::EdgeType::{self, *};
        let link = |edge_type: EdgeType, source_owned: bool, target_owned: bool| LinkRef {
            id: Uuid::nil(),
            edge_type,
            source: Uuid::from_u128(1),
            target: Uuid::from_u128(2),
            source_owned,
            target_owned,
        };
        // The child's owner may free a home and relate it, not reference it.
        let freed = link(Contains, false, true);
        assert!(freed.removable());
        assert!(freed.convertible_to(&RelatedTo));
        assert!(!freed.convertible_to(&References));
        // The parent's owner may do all three.
        let parent = link(Contains, true, false);
        assert!(parent.removable() && parent.convertible_to(&References));
        // A dependency is its source's alone.
        let dep = link(DependsOn, false, true);
        assert!(!dep.removable() && !dep.convertible_to(&RelatedTo));
        assert!(link(DependsOn, true, false).removable());
        // Never to a structural type.
        assert!(!parent.convertible_to(&DependsOn));
    }

    #[test]
    fn links_are_capped_one_past_related_and_omitted_when_empty() {
        let l = |n: u128| LinkRef {
            id: Uuid::from_u128(n),
            edge_type: crate::edge::EdgeType::Contains,
            source: Uuid::nil(),
            target: Uuid::nil(),
            source_owned: true,
            target_owned: true,
        };
        let v = item(1).with_links((0..(RELATED_CAP as u128 + 9)).map(l).collect());
        assert_eq!(v.links.len(), RELATED_CAP + 1);
        let json = serde_json::to_value(item(2)).expect("ser");
        assert!(json.get("links").is_none());
    }

    #[test]
    fn graph_rule_counts_counts_each_rule_like_the_report() {
        use crate::edge::EdgeType::*;
        use crate::node::NodeType::{self, *};
        let id = Uuid::from_u128;
        let kinds: std::collections::HashMap<Uuid, NodeType> = [
            (id(1), Area),
            (id(2), Area),
            (id(3), Project),
            (id(4), Reference),
            (id(5), Article),
            (id(6), Project),
        ]
        .into_iter()
        .collect();
        let edges = vec![
            (id(1), id(3), Contains),  // fine
            (id(2), id(3), Contains),  // project 3 has two homes
            (id(1), id(4), Contains),  // a contained Reference
            (id(5), id(6), Contains),  // an Article can't be a home
            (id(3), id(1), DependsOn), // on an Area
            (id(3), id(6), DependsOn), // fine
            (id(6), id(3), DependsOn), // closes a loop with the line above
            (id(3), id(6), RelatedTo),
            (id(6), id(3), RelatedTo), // both ways: merged on restore, not a rule
            (id(5), id(4), DerivedFrom), // fine
            (id(1), id(9), Contains),  // unknown end: skipped
            (id(4), id(5), WikiLink),  // mentions are never a violation
        ];
        assert_eq!(
            graph_rule_counts(&kinds, &edges),
            vec![
                (RuleId::MultipleHomes, 1),
                (RuleId::ContainedReference, 1),
                (RuleId::ContainmentNotAllowed, 1),
                (RuleId::DependsOnArea, 1),
                (RuleId::DependsOnCycle, 1),
            ]
        );
        assert!(graph_rule_counts(&kinds, &[]).is_empty());
    }
}
