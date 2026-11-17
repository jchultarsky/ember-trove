//! A node's links, grouped by what they mean (S7a; ONTOLOGY §6, §14).
//!
//! The node page used to list every link in one run: an arrow, the
//! type's name, a title. The ontology gives each type and direction a
//! meaning of its own: what a node draws on and where it is used (§6.3),
//! what it came from and what came from it (§6.5), and so on. This is
//! that reading, as pure functions over the one answer of
//! `GET /nodes/{id}/edges`.
//!
//! Nothing here decides who sees what: the answer holds only links whose
//! two ends its reader can view (`repo::edge`), so a group, a count or a
//! notice built from it can only ever understate (§14).

use crate::edge::{EdgeEnd, EdgeType, EdgeWithTitles};
use crate::id::{EdgeId, NodeId};
use crate::node::NodeType;

/// One meaning a link has for the node whose page it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// The node that contains this one: its one home (§6.1).
    Home,
    /// What this node contains.
    Contains,
    /// What this node draws on (§6.3). An Article's bibliography.
    Sources,
    /// The nodes that draw on this one: the reverse index of §6.3.
    UsedIn,
    /// What this node came from (§6.5).
    DerivedFrom,
    /// What came from this node (§6.5).
    Descendants,
    /// What this node can't proceed without (§6.4).
    DependsOn,
    /// What can't proceed without this node.
    NeededBy,
    /// `related_to`, which has no direction (§6.2).
    Related,
    /// Nodes this one's text names with `[[Title]]` (§6.6).
    Mentions,
    /// Nodes whose text names this one.
    MentionedIn,
}

impl Group {
    /// Every group, in the order the page shows them.
    pub const ALL: [Group; 11] = [
        Group::Home,
        Group::Contains,
        Group::Sources,
        Group::UsedIn,
        Group::DerivedFrom,
        Group::Descendants,
        Group::DependsOn,
        Group::NeededBy,
        Group::Related,
        Group::Mentions,
        Group::MentionedIn,
    ];

    /// The group's heading on a node of `kind`. An Article's sources are
    /// its bibliography (§6.3); elsewhere they are just its sources.
    #[must_use]
    pub fn heading(self, kind: &NodeType) -> &'static str {
        match self {
            Group::Home => "Home",
            Group::Contains => "Contains",
            Group::Sources if *kind == NodeType::Article => "Bibliography",
            Group::Sources => "Sources",
            Group::UsedIn => "Used in",
            Group::DerivedFrom => "Derived from",
            Group::Descendants => "Descendants",
            Group::DependsOn => "Depends on",
            Group::NeededBy => "Needed by",
            Group::Related => "Related",
            Group::Mentions => "Mentions",
            Group::MentionedIn => "Mentioned in",
        }
    }

    /// A stable name for the DOM and tests: `connections-<slug>`.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Group::Home => "home",
            Group::Contains => "contains",
            Group::Sources => "sources",
            Group::UsedIn => "used-in",
            Group::DerivedFrom => "derived-from",
            Group::Descendants => "descendants",
            Group::DependsOn => "depends-on",
            Group::NeededBy => "needed-by",
            Group::Related => "related",
            Group::Mentions => "mentions",
            Group::MentionedIn => "mentioned-in",
        }
    }
}

/// The group of a link of `edge_type` seen from one of its ends:
/// `outgoing` when the page's node is the link's source. Every type and
/// direction has exactly one group, so no link is left off the page.
#[must_use]
pub fn group_of(edge_type: &EdgeType, outgoing: bool) -> Group {
    match (edge_type, outgoing) {
        // For `contains` the source is the home and the target the child.
        (EdgeType::Contains, true) => Group::Contains,
        (EdgeType::Contains, false) => Group::Home,
        (EdgeType::References, true) => Group::Sources,
        (EdgeType::References, false) => Group::UsedIn,
        (EdgeType::DerivedFrom, true) => Group::DerivedFrom,
        (EdgeType::DerivedFrom, false) => Group::Descendants,
        (EdgeType::DependsOn, true) => Group::DependsOn,
        (EdgeType::DependsOn, false) => Group::NeededBy,
        (EdgeType::RelatedTo, _) => Group::Related,
        (EdgeType::WikiLink, true) => Group::Mentions,
        (EdgeType::WikiLink, false) => Group::MentionedIn,
    }
}

/// One row of a group: the node at the link's other end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub edge_id: EdgeId,
    pub edge_type: EdgeType,
    pub other_id: NodeId,
    pub other_title: String,
    /// Its kind and standing; `None` from an answer cached before S7a.
    pub other: Option<EdgeEnd>,
    pub label: Option<String>,
    /// Whether the reader may remove the link (the server's answer).
    pub may_remove: bool,
}

impl Connection {
    /// Whether the other node is still in use (`node::is_live`). One
    /// whose standing isn't known is taken as live: nothing is said
    /// against it.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.other.as_ref().is_none_or(EdgeEnd::is_live)
    }

    /// What to say beside a node that is not live: "Archived", or a
    /// closed Project's state. `None` for a live one.
    #[must_use]
    pub fn standing(&self) -> Option<&'static str> {
        let other = self.other.as_ref()?;
        if other.status == crate::node::NodeStatus::Archived {
            return Some("Archived");
        }
        other
            .project_state
            .filter(|s| s.is_terminal())
            .map(|s| s.label())
    }
}

/// `edges`, as the page of `node` shows them: the non-empty groups in
/// [`Group::ALL`] order, each with its live rows first, then the rest,
/// each part by title (ignoring case), then by id so the order is
/// stable. A link that doesn't touch `node` is left out.
#[must_use]
pub fn grouped(node: NodeId, edges: Vec<EdgeWithTitles>) -> Vec<(Group, Vec<Connection>)> {
    let mut rows: Vec<(Group, Connection)> = edges
        .into_iter()
        .filter_map(|e| {
            let outgoing = e.source_id == node;
            if !outgoing && e.target_id != node {
                return None;
            }
            let group = group_of(&e.edge_type, outgoing);
            let (other_id, other_title, other) = if outgoing {
                (e.target_id, e.target_title, e.target_node)
            } else {
                (e.source_id, e.source_title, e.source_node)
            };
            Some((
                group,
                Connection {
                    edge_id: e.id,
                    edge_type: e.edge_type,
                    other_id,
                    other_title,
                    other,
                    label: e.label.filter(|l| !l.trim().is_empty()),
                    may_remove: e.may_remove,
                },
            ))
        })
        .collect();
    rows.sort_by(|(ga, a), (gb, b)| {
        ga.cmp(gb)
            .then_with(|| b.is_live().cmp(&a.is_live()))
            .then_with(|| {
                a.other_title
                    .to_lowercase()
                    .cmp(&b.other_title.to_lowercase())
            })
            .then_with(|| a.edge_id.cmp(&b.edge_id))
    });
    let mut out: Vec<(Group, Vec<Connection>)> = Vec::new();
    for (group, row) in rows {
        match out.last_mut() {
            Some((last, list)) if *last == group => list.push(row),
            _ => out.push((group, vec![row])),
        }
    }
    out
}

/// How many live nodes still use `node` as a source, among `edges`: the
/// archive notice's count (§6.3). It is the live rows of "Used in", so
/// the notice and the list can't disagree.
#[must_use]
pub fn live_users(node: NodeId, edges: &[EdgeWithTitles]) -> usize {
    edges
        .iter()
        .filter(|e| e.edge_type == EdgeType::References && e.target_id == node)
        .filter(|e| e.source_node.as_ref().is_none_or(EdgeEnd::is_live))
        .count()
}

/// The notice shown when a node is archived while live nodes still use
/// it as a source (§6.3: a warning, not a block). `None` when none does.
#[must_use]
pub fn archive_notice(live_users: usize) -> Option<String> {
    match live_users {
        0 => None,
        1 => Some("1 node in use still draws on this as a source. The link stays.".to_string()),
        n => Some(format!(
            "{n} nodes in use still draw on this as a source. The links stay."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::NodeStatus;
    use crate::project_state::ProjectState;
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    fn id(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    fn end(status: NodeStatus, state: Option<ProjectState>) -> Option<EdgeEnd> {
        Some(EdgeEnd {
            node_type: if state.is_some() {
                NodeType::Project
            } else {
                NodeType::Reference
            },
            status,
            project_state: state,
        })
    }

    fn live() -> Option<EdgeEnd> {
        end(NodeStatus::Published, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn edge(
        n: u128,
        source: u128,
        target: u128,
        edge_type: EdgeType,
        source_title: &str,
        target_title: &str,
        source_node: Option<EdgeEnd>,
        target_node: Option<EdgeEnd>,
    ) -> EdgeWithTitles {
        EdgeWithTitles {
            id: EdgeId(Uuid::from_u128(n)),
            source_id: id(source),
            source_title: source_title.to_string(),
            target_id: id(target),
            target_title: target_title.to_string(),
            edge_type,
            label: None,
            created_at: DateTime::<Utc>::UNIX_EPOCH,
            source_node,
            target_node,
            may_remove: false,
        }
    }

    /// No link may be left off the page: every type, from either end, has
    /// a group, and only `related_to` (which has no direction) shares one.
    #[test]
    fn every_type_and_direction_has_exactly_one_group() {
        let mut seen = Vec::new();
        for edge_type in EdgeType::ALL {
            let (out, back) = (group_of(edge_type, true), group_of(edge_type, false));
            assert_eq!(
                out == back,
                edge_type.is_symmetric(),
                "{edge_type:?}: only a link with no direction reads the same from both ends"
            );
            seen.extend([out, back]);
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen, Group::ALL, "every group is used, and no other");
        // The readings the ontology names (§6.3, §6.5).
        assert_eq!(group_of(&EdgeType::References, false), Group::UsedIn);
        assert_eq!(group_of(&EdgeType::References, true), Group::Sources);
        assert_eq!(group_of(&EdgeType::DerivedFrom, true), Group::DerivedFrom);
        assert_eq!(group_of(&EdgeType::DerivedFrom, false), Group::Descendants);
        // For `contains` the source is the home.
        assert_eq!(group_of(&EdgeType::Contains, false), Group::Home);
    }

    #[test]
    fn an_articles_sources_are_its_bibliography() {
        assert_eq!(Group::Sources.heading(&NodeType::Article), "Bibliography");
        for kind in [NodeType::Project, NodeType::Area, NodeType::Reference] {
            assert_eq!(Group::Sources.heading(&kind), "Sources", "{kind:?}");
        }
        assert_eq!(Group::UsedIn.heading(&NodeType::Reference), "Used in");
        // Headings and slugs are all different.
        let mut headings: Vec<_> = Group::ALL
            .iter()
            .map(|g| g.heading(&NodeType::Article))
            .collect();
        let mut slugs: Vec<_> = Group::ALL.iter().map(|g| g.slug()).collect();
        headings.sort_unstable();
        headings.dedup();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!((headings.len(), slugs.len()), (11, 11));
    }

    #[test]
    fn links_are_grouped_in_page_order_with_live_nodes_first() {
        let me = 1;
        let closed = end(NodeStatus::Published, Some(ProjectState::Complete));
        let parked = end(NodeStatus::Published, Some(ProjectState::Parked));
        let archived = end(NodeStatus::Archived, None);
        let edges = vec![
            // Three nodes that use this one: one archived, one closed.
            edge(
                10,
                20,
                me,
                EdgeType::References,
                "zeta",
                "me",
                live(),
                live(),
            ),
            edge(
                11,
                21,
                me,
                EdgeType::References,
                "Alpha",
                "me",
                archived.clone(),
                live(),
            ),
            edge(
                12,
                22,
                me,
                EdgeType::References,
                "beta",
                "me",
                closed,
                live(),
            ),
            edge(
                13,
                23,
                me,
                EdgeType::References,
                "gamma",
                "me",
                parked,
                live(),
            ),
            // What this one draws on, came from, and is filed under.
            edge(
                14,
                me,
                30,
                EdgeType::References,
                "me",
                "a source",
                live(),
                live(),
            ),
            edge(
                15,
                me,
                31,
                EdgeType::DerivedFrom,
                "me",
                "an origin",
                live(),
                live(),
            ),
            edge(
                16,
                32,
                me,
                EdgeType::Contains,
                "a home",
                "me",
                live(),
                live(),
            ),
            edge(17, 33, me, EdgeType::RelatedTo, "kin", "me", live(), live()),
            // Not this node's at all.
            edge(18, 40, 41, EdgeType::References, "x", "y", live(), live()),
        ];
        let groups = grouped(id(me), edges);
        let shape: Vec<(Group, Vec<&str>)> = groups
            .iter()
            .map(|(g, rows)| (*g, rows.iter().map(|r| r.other_title.as_str()).collect()))
            .collect();
        assert_eq!(
            shape,
            vec![
                (Group::Home, vec!["a home"]),
                (Group::Sources, vec!["a source"]),
                // Live first (a parked Project is live), then the rest;
                // each part by title, whatever its case.
                (Group::UsedIn, vec!["gamma", "zeta", "Alpha", "beta"]),
                (Group::DerivedFrom, vec!["an origin"]),
                (Group::Related, vec!["kin"]),
            ]
        );
        let used = &groups[2].1;
        assert_eq!(used[0].standing(), None);
        assert_eq!(used[2].standing(), Some("Archived"));
        assert_eq!(used[3].standing(), Some("Complete"));
        assert!(grouped(id(me), Vec::new()).is_empty());
    }

    #[test]
    fn the_archive_notice_counts_the_live_rows_of_used_in() {
        let me = 1;
        let edges = vec![
            edge(10, 20, me, EdgeType::References, "a", "me", live(), live()),
            edge(
                11,
                21,
                me,
                EdgeType::References,
                "b",
                "me",
                end(NodeStatus::Archived, None),
                live(),
            ),
            edge(
                12,
                22,
                me,
                EdgeType::References,
                "c",
                "me",
                end(NodeStatus::Published, Some(ProjectState::Cancelled)),
                live(),
            ),
            // A source of this node is not a user of it.
            edge(13, me, 23, EdgeType::References, "me", "d", live(), live()),
            // Another kind of link is not a use as a source.
            edge(14, 24, me, EdgeType::DerivedFrom, "e", "me", live(), live()),
            // Standing unknown (an older cached answer): counted as live.
            edge(15, 25, me, EdgeType::References, "f", "me", None, None),
        ];
        assert_eq!(live_users(id(me), &edges), 2);
        let live_rows = grouped(id(me), edges)
            .into_iter()
            .find(|(g, _)| *g == Group::UsedIn)
            .map(|(_, rows)| rows.iter().filter(|r| r.is_live()).count());
        assert_eq!(live_rows, Some(2), "the notice is the list's own count");

        assert_eq!(archive_notice(0), None);
        assert_eq!(
            archive_notice(1).as_deref(),
            Some("1 node in use still draws on this as a source. The link stays.")
        );
        assert!(
            archive_notice(4).is_some_and(|n| n.starts_with("4 nodes in use still draw on this"))
        );
    }

    #[test]
    fn an_empty_label_is_no_label() {
        let mut e = edge(10, 1, 2, EdgeType::References, "me", "x", live(), live());
        e.label = Some("  ".to_string());
        e.may_remove = true;
        let groups = grouped(id(1), vec![e]);
        assert_eq!(groups[0].1[0].label, None);
        assert!(groups[0].1[0].may_remove);
    }
}
