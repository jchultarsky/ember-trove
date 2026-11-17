//! Blockers: what a node, or a task, waits for (S7b; ONTOLOGY §6.4,
//! §9.3, §14).
//!
//! A `depends_on` link is a hard prerequisite, and "unsatisfied
//! dependencies are displayed as blockers (including transitively,
//! display-only)". Nothing here prevents, reorders or changes anything:
//! these are the rules for what is *said*.
//!
//! - [`satisfies_dependency`]: when a dependency on a node is met.
//! - [`is_blocked`]: when a node says "Blocked".
//! - [`passes_on`]: when a chain goes on through a node.
//! - [`chain`]: a walk's links, as "also waits for …, through …".
//!
//! Nothing here decides who sees what. Every answer these read is cut
//! to its reader first (`repo::edge`, `repo::task_dependency`): a link
//! with an end they can't view is in no answer, so it is in no count and
//! no chain, and a chain stops at a node they can't see (§14).

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::edge::{EdgeEnd, EdgeType, EdgeWithTitles};
use crate::id::NodeId;
use crate::node::{NodeStatus, NodeType, is_live};
use crate::project_state::ProjectState;

/// A walk reads up to this many links: the node's own unmet
/// dependencies count among them, then the chain behind them. When
/// there are more, the answer says so ([`NodeBlockers::truncated`]) and
/// holds exactly this many. The count speaks only of links their reader
/// can see.
pub const CHAIN_LIMIT: usize = 100;

/// Whether a dependency on a node of this kind and standing is met
/// (§6.4):
///
/// | Target | Met when |
/// |---|---|
/// | Project | its state is *complete*; never when cancelled |
/// | Article | it is Published |
/// | Reference | it is not archived: the material is available |
/// | Area | never: an Area is a standard to maintain, not a thing that finishes |
///
/// A Project is judged by its state alone: complete and archived is
/// met, active and archived is not. An archived Article is not
/// Published (the lifecycle is one field), so it is not met. A Project
/// with no state (older data never has one) fails closed.
#[must_use]
pub fn satisfies_dependency(
    kind: &NodeType,
    status: &NodeStatus,
    project_state: Option<ProjectState>,
) -> bool {
    match kind {
        NodeType::Project => project_state == Some(ProjectState::Complete),
        NodeType::Article => *status == NodeStatus::Published,
        NodeType::Reference => *status != NodeStatus::Archived,
        NodeType::Area => false,
    }
}

/// Whether a node still waits for anything: it is in use ([`is_live`])
/// and is not itself what a dependency on it asks for
/// ([`satisfies_dependency`]). An active or parked Project, a draft
/// Article and an Area do. A complete Project, a Published Article and a
/// Reference do not: they are finished or available, so nothing of
/// theirs waits to proceed, whatever they still link to. Nor does a
/// cancelled or an archived node: it will not proceed.
///
/// One notion for both ends: a node that does not still wait is not
/// blocked ([`is_blocked`]), and a chain does not go on through it
/// ([`passes_on`]).
#[must_use]
pub fn still_waits(
    kind: &NodeType,
    status: &NodeStatus,
    project_state: Option<ProjectState>,
) -> bool {
    is_live(status, project_state) && !satisfies_dependency(kind, status, project_state)
}

/// Whether a chain goes on through a node: what it waits for is shown
/// as something its dependents wait for too. Only a node that
/// [`still_waits`], and never an Area: an Area is not a valid target at
/// all (an old link to one is listed in Data health), so nothing is read
/// through it.
#[must_use]
pub fn passes_on(
    kind: &NodeType,
    status: &NodeStatus,
    project_state: Option<ProjectState>,
) -> bool {
    still_waits(kind, status, project_state) && *kind != NodeType::Area
}

/// A kind, a status and a Project state in the database's words; `""`
/// is "no state".
pub type Triple = (&'static str, &'static str, &'static str);

/// Every (kind, status, state) for which `rule` holds. The SQL for a
/// rule is built from this list (`repo::node::satisfies_dependency`,
/// `passes_on`), as an `IN` over whole triples: it can't name a
/// different set than the function, and a NULL state can't silently
/// drop a row the way `NOT (state IN (…))` does.
fn triples(rule: fn(&NodeType, &NodeStatus, Option<ProjectState>) -> bool) -> Vec<Triple> {
    let states = std::iter::once(None).chain(ProjectState::ALL.iter().copied().map(Some));
    let mut out = Vec::new();
    for state in states {
        for kind in NodeType::ALL {
            for status in NodeStatus::ALL {
                if rule(kind, status, state) {
                    out.push((
                        kind.as_str(),
                        status.as_str(),
                        state.map_or("", |s| s.as_str()),
                    ));
                }
            }
        }
    }
    out
}

/// The triples [`satisfies_dependency`] accepts.
#[must_use]
pub fn satisfying_triples() -> Vec<Triple> {
    triples(satisfies_dependency)
}

/// The triples [`passes_on`] accepts.
#[must_use]
pub fn passing_triples() -> Vec<Triple> {
    triples(passes_on)
}

/// The triples [`still_waits`] accepts.
#[must_use]
pub fn waiting_triples() -> Vec<Triple> {
    triples(still_waits)
}

/// [`satisfies_dependency`] for one end of a link.
#[must_use]
pub fn met(end: &EdgeEnd) -> bool {
    satisfies_dependency(&end.node_type, &end.status, end.project_state)
}

/// Whether a node says "Blocked": it [`still_waits`], and `unmet` of
/// its dependencies, among those its reader can see, are not met. A
/// node that is archived or closed is never blocked, and neither is one
/// that is itself finished or available (a complete Project, a Published
/// Article, a Reference): nothing of theirs is waiting to proceed. The
/// mirror of `task_dependency::is_blocked`, where a finished task is
/// never blocked.
///
/// Only the direct dependencies matter: a chain adds to what a node
/// waits for, and never changes whether it waits.
#[must_use]
pub fn is_blocked(
    kind: &NodeType,
    status: &NodeStatus,
    project_state: Option<ProjectState>,
    unmet: u32,
) -> bool {
    still_waits(kind, status, project_state) && unmet > 0
}

/// The chip's tooltip.
#[must_use]
pub fn blocked_label(unmet: u32) -> String {
    if unmet == 1 {
        "Waits for 1 node that is not finished".to_string()
    } else {
        format!("Waits for {unmet} nodes that are not finished")
    }
}

/// What a "Depends on" row says of its target: "Met", or the standing
/// that keeps it from being met. A cancelled Project, an archived node
/// and an Area will not let their dependents go: the row says which, and
/// the link is its person's to remove.
#[must_use]
pub fn standing(end: &EdgeEnd) -> &'static str {
    if met(end) {
        return "Met";
    }
    match end.node_type {
        NodeType::Area => "Never met",
        NodeType::Project => match end.project_state {
            Some(ProjectState::Cancelled) => ProjectState::Cancelled.label(),
            _ if end.status == NodeStatus::Archived => NodeStatus::Archived.label(),
            Some(state) => state.label(),
            None => "No state",
        },
        NodeType::Article | NodeType::Reference => end.status.label(),
    }
}

/// How many of `node`'s own dependencies are not met, in the answer of
/// `GET /nodes/{id}/edges`: its outgoing `depends_on` links whose target
/// is not met. The page reads the chip from this once the links are
/// loaded; the server's count (`ViewedNode::blocked_by`) is the same
/// rule in SQL, and a Postgres test holds the two equal for every
/// reader.
///
/// A target whose standing an old cached answer doesn't carry says
/// nothing: it is not counted.
#[must_use]
pub fn direct(node: NodeId, edges: &[EdgeWithTitles]) -> u32 {
    let n = edges
        .iter()
        .filter(|e| e.edge_type == EdgeType::DependsOn && e.source_id == node)
        .filter(|e| e.target_node.as_ref().is_some_and(|end| !met(end)))
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// One link of a walk: `from` waits for `to`, and `item` is what is
/// shown of `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop<Id, T> {
    pub from: Id,
    pub to: Id,
    pub item: T,
}

/// One thing a chain shows: reached from the start at `depth` (2 or
/// more), through the start's own dependency `via`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chained<Id, T> {
    pub id: Id,
    pub item: T,
    pub depth: u32,
    pub via: Id,
}

/// The chain of `start`, from the links of a walk: everything it waits
/// for **beyond its own dependencies**, each once, with the dependency
/// of its own that leads there. Nodes and tasks share it.
///
/// - Breadth-first, so each thing is listed at its shortest distance.
/// - Where two dependencies lead to the same thing (a diamond), it is
///   listed under the one that sorts first by `sort_key`, then by id:
///   the order of the input never changes the answer.
/// - A direct dependency is never listed again, even when another one
///   also leads to it; the start never appears in its own chain.
/// - A link not reachable from the start is ignored, and a loop (older
///   data can hold one) ends: each thing is visited once.
#[must_use]
pub fn chain<Id, T, K>(
    start: Id,
    hops: Vec<Hop<Id, T>>,
    sort_key: impl Fn(&T) -> K,
) -> Vec<Chained<Id, T>>
where
    Id: Copy + Eq + Hash + Ord,
    T: Clone,
    K: Ord,
{
    let mut next: HashMap<Id, Vec<(Id, T)>> = HashMap::new();
    for hop in hops {
        next.entry(hop.from).or_default().push((hop.to, hop.item));
    }
    for children in next.values_mut() {
        children.sort_by(|(a_id, a), (b_id, b)| {
            sort_key(a).cmp(&sort_key(b)).then_with(|| a_id.cmp(b_id))
        });
        children.dedup_by(|(a, _), (b, _)| a == b);
    }

    let mut seen: HashSet<Id> = HashSet::from([start]);
    // (the thing, its depth, the direct dependency that leads to it)
    let mut queue: VecDeque<(Id, u32, Id)> = VecDeque::new();
    for (id, _) in next.get(&start).into_iter().flatten() {
        if seen.insert(*id) {
            queue.push_back((*id, 1, *id));
        }
    }
    let mut out = Vec::new();
    while let Some((at, depth, via)) = queue.pop_front() {
        for (id, item) in next.get(&at).into_iter().flatten() {
            if seen.insert(*id) {
                let depth = depth.saturating_add(1);
                out.push(Chained {
                    id: *id,
                    item: item.clone(),
                    depth,
                    via,
                });
                queue.push_back((*id, depth, via));
            }
        }
    }
    out
}

/// A node that another waits for through a chain, as its reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Blocker {
    pub id: NodeId,
    pub title: String,
    /// Its kind and standing: never met, or it would not be listed.
    pub node: EdgeEnd,
    /// How many links away: 2 or more (a direct dependency is in the
    /// node's own "Depends on" list).
    pub depth: u32,
    /// The node's own dependency that leads here, and its title.
    pub via: NodeId,
    pub via_title: String,
}

/// `GET /nodes/{id}/blockers`: what the node's unmet dependencies wait
/// for in turn, for one reader.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NodeBlockers {
    pub also: Vec<Blocker>,
    /// The chain has more links than were read ([`CHAIN_LIMIT`]).
    #[serde(default)]
    pub truncated: bool,
}

/// One row of a node walk, as the repo reads it: a link, and the node
/// it leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeHop {
    pub from: NodeId,
    pub to: NodeId,
    pub title: String,
    pub node: EdgeEnd,
}

/// The answer for `start` from the rows of its walk. `truncated` is the
/// repo's: it read one row more than [`CHAIN_LIMIT`].
#[must_use]
pub fn node_blockers(start: NodeId, rows: Vec<NodeHop>, truncated: bool) -> NodeBlockers {
    let titles: HashMap<NodeId, String> = rows.iter().map(|r| (r.to, r.title.clone())).collect();
    let hops = rows
        .into_iter()
        .map(|r| Hop {
            from: r.from,
            to: r.to,
            item: (r.title, r.node),
        })
        .collect();
    let also = chain(start, hops, |(title, _)| title.to_lowercase())
        .into_iter()
        .map(|c| Blocker {
            id: c.id,
            title: c.item.0,
            node: c.item.1,
            depth: c.depth,
            via_title: titles.get(&c.via).cloned().unwrap_or_default(),
            via: c.via,
        })
        .collect();
    NodeBlockers { also, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::EdgeId;
    use uuid::Uuid;

    use NodeStatus::{Archived, Draft, Published};
    use NodeType::{Area, Article, Project, Reference};
    use ProjectState::{Active, Cancelled, Complete, Parked};

    fn id(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    /// Every kind, status and state, the impossible combinations too (a
    /// Project with no state, an Article with one): the rules answer for
    /// all 60, and the word lists their SQL is built from are held to
    /// them here on all 60. The Postgres test holds the SQL itself on
    /// the 21 a row can store.
    fn grid() -> Vec<(NodeType, NodeStatus, Option<ProjectState>)> {
        let mut out = Vec::new();
        for kind in NodeType::ALL {
            for status in NodeStatus::ALL {
                out.push((kind.clone(), status.clone(), None));
                for state in ProjectState::ALL {
                    out.push((kind.clone(), status.clone(), Some(*state)));
                }
            }
        }
        out
    }

    #[test]
    fn a_dependency_is_met_as_the_ontology_table_says() {
        // Project: complete, whatever its lifecycle status; never
        // cancelled, active or parked; never with no state.
        for status in NodeStatus::ALL {
            assert!(satisfies_dependency(&Project, status, Some(Complete)));
            for state in [Active, Parked, Cancelled] {
                assert!(!satisfies_dependency(&Project, status, Some(state)));
            }
            assert!(!satisfies_dependency(&Project, status, None));
        }
        // Article: Published only. An archived one is not Published.
        assert!(satisfies_dependency(&Article, &Published, None));
        assert!(!satisfies_dependency(&Article, &Draft, None));
        assert!(!satisfies_dependency(&Article, &Archived, None));
        // Reference: it exists and is not archived.
        assert!(satisfies_dependency(&Reference, &Draft, None));
        assert!(satisfies_dependency(&Reference, &Published, None));
        assert!(!satisfies_dependency(&Reference, &Archived, None));
        // Area: never.
        for (kind, status, state) in grid() {
            if kind == Area {
                assert!(!satisfies_dependency(&kind, &status, state));
            }
        }
    }

    #[test]
    fn a_chain_goes_on_only_through_what_is_unmet_and_can_still_move() {
        for (kind, status, state) in grid() {
            let goes_on = passes_on(&kind, &status, state);
            // Never through a met node, one out of use, or an Area.
            if satisfies_dependency(&kind, &status, state)
                || !is_live(&status, state)
                || kind == Area
            {
                assert!(!goes_on, "{kind:?} {status:?} {state:?}");
            } else {
                assert!(goes_on, "{kind:?} {status:?} {state:?}");
            }
            // It goes on only through a node that still waits, and
            // through every one of those but an Area.
            assert_eq!(
                goes_on,
                still_waits(&kind, &status, state) && kind != Area,
                "{kind:?} {status:?} {state:?}"
            );
        }
        // What still waits, by name.
        assert!(still_waits(&Project, &Draft, Some(Active)));
        assert!(still_waits(&Project, &Published, Some(Parked)));
        assert!(still_waits(&Article, &Draft, None));
        assert!(still_waits(&Area, &Draft, None), "an Area never finishes");
        assert!(!still_waits(&Project, &Draft, Some(Complete)), "finished");
        assert!(!still_waits(&Article, &Published, None), "finished");
        assert!(!still_waits(&Reference, &Draft, None), "available");
        assert!(!still_waits(&Reference, &Archived, None), "out of use");
        assert!(!still_waits(&Project, &Draft, Some(Cancelled)), "closed");
        assert!(!still_waits(&Project, &Archived, Some(Active)), "archived");
        // The cases by name.
        assert!(passes_on(&Project, &Draft, Some(Active)));
        assert!(passes_on(&Project, &Draft, Some(Parked)));
        assert!(passes_on(&Article, &Draft, None));
        assert!(!passes_on(&Project, &Draft, Some(Complete)), "met");
        assert!(!passes_on(&Project, &Draft, Some(Cancelled)), "closed");
        assert!(!passes_on(&Project, &Archived, Some(Active)), "archived");
        assert!(!passes_on(&Article, &Archived, None), "archived");
        assert!(!passes_on(&Reference, &Archived, None), "archived");
        assert!(!passes_on(&Area, &Draft, None), "an Area never finishes");
    }

    #[test]
    fn the_sql_word_lists_are_the_rules() {
        let accepts = |list: &[Triple], kind: &NodeType, status: &NodeStatus, state| {
            list.contains(&(
                kind.as_str(),
                status.as_str(),
                Option::map_or(state, "", |s: ProjectState| s.as_str()),
            ))
        };
        let (satisfying, passing) = (satisfying_triples(), passing_triples());
        let waiting = waiting_triples();
        for (kind, status, state) in grid() {
            assert_eq!(
                accepts(&waiting, &kind, &status, state),
                still_waits(&kind, &status, state),
                "{kind:?} {status:?} {state:?}"
            );
            assert_eq!(
                accepts(&satisfying, &kind, &status, state),
                satisfies_dependency(&kind, &status, state),
                "{kind:?} {status:?} {state:?}"
            );
            assert_eq!(
                accepts(&passing, &kind, &status, state),
                passes_on(&kind, &status, state),
                "{kind:?} {status:?} {state:?}"
            );
        }
        // No triple is listed twice, and none satisfies and passes on.
        let unique: HashSet<&Triple> = satisfying.iter().collect();
        assert_eq!(unique.len(), satisfying.len());
        assert!(satisfying.iter().all(|t| !passing.contains(t)));
    }

    #[test]
    fn a_node_is_blocked_only_while_it_still_waits() {
        assert!(is_blocked(&Project, &Draft, Some(Active), 1));
        assert!(
            is_blocked(&Project, &Draft, Some(Parked), 2),
            "parked is in use"
        );
        assert!(is_blocked(&Article, &Draft, None, 1));
        assert!(is_blocked(&Area, &Published, None, 1));
        // Nothing unmet: not blocked.
        assert!(!is_blocked(&Project, &Draft, Some(Active), 0));
        // Out of use: nothing waits to proceed.
        assert!(!is_blocked(&Article, &Archived, None, 3));
        assert!(!is_blocked(&Project, &Draft, Some(Cancelled), 3));
        assert!(!is_blocked(&Project, &Archived, Some(Active), 3));
        // Itself finished or available: its dependents read it as met,
        // so it can't say it is still waiting.
        assert!(!is_blocked(&Project, &Draft, Some(Complete), 3));
        assert!(!is_blocked(&Article, &Published, None, 3));
        assert!(!is_blocked(&Reference, &Draft, None, 3));
        // On the whole grid: never blocked while a dependency on it
        // would be met, and never while out of use.
        for (kind, status, state) in grid() {
            if satisfies_dependency(&kind, &status, state) || !is_live(&status, state) {
                assert!(
                    !is_blocked(&kind, &status, state, 5),
                    "{kind:?} {status:?} {state:?}"
                );
            }
        }
        assert_eq!(blocked_label(1), "Waits for 1 node that is not finished");
        assert_eq!(blocked_label(3), "Waits for 3 nodes that are not finished");
    }

    fn end(kind: NodeType, status: NodeStatus, state: Option<ProjectState>) -> EdgeEnd {
        EdgeEnd {
            node_type: kind,
            status,
            project_state: state,
        }
    }

    #[test]
    fn a_depends_on_row_says_met_or_what_keeps_it_from_being_met() {
        assert_eq!(standing(&end(Project, Draft, Some(Complete))), "Met");
        // Complete and archived is still met: the state decides.
        assert_eq!(standing(&end(Project, Archived, Some(Complete))), "Met");
        assert_eq!(standing(&end(Project, Draft, Some(Active))), "Active");
        assert_eq!(standing(&end(Project, Draft, Some(Parked))), "Parked");
        assert_eq!(standing(&end(Project, Draft, Some(Cancelled))), "Cancelled");
        // Cancelled says so even when archived as well.
        assert_eq!(
            standing(&end(Project, Archived, Some(Cancelled))),
            "Cancelled"
        );
        assert_eq!(standing(&end(Project, Archived, Some(Active))), "Archived");
        assert_eq!(standing(&end(Project, Draft, None)), "No state");
        assert_eq!(standing(&end(Article, Published, None)), "Met");
        assert_eq!(standing(&end(Article, Draft, None)), "Draft");
        assert_eq!(standing(&end(Article, Archived, None)), "Archived");
        assert_eq!(standing(&end(Reference, Draft, None)), "Met");
        assert_eq!(standing(&end(Reference, Archived, None)), "Archived");
        assert_eq!(standing(&end(Area, Published, None)), "Never met");
        // Never "Met" for something that is not met, on the whole grid.
        for (kind, status, state) in grid() {
            let e = end(kind, status, state);
            assert_eq!(standing(&e) == "Met", met(&e), "{e:?}");
        }
    }

    fn link(
        n: u128,
        source: u128,
        target: u128,
        edge_type: EdgeType,
        target_node: Option<EdgeEnd>,
    ) -> EdgeWithTitles {
        EdgeWithTitles {
            id: EdgeId(Uuid::from_u128(0xe000 + n)),
            source_id: id(source),
            target_id: id(target),
            source_title: format!("node {source}"),
            target_title: format!("node {target}"),
            edge_type,
            label: None,
            created_at: chrono::DateTime::UNIX_EPOCH,
            source_node: None,
            target_node,
            may_remove: false,
        }
    }

    #[test]
    fn the_direct_count_is_the_nodes_own_unmet_dependencies() {
        let active = || Some(end(Project, Draft, Some(Active)));
        let complete = || Some(end(Project, Draft, Some(Complete)));
        let edges = vec![
            link(1, 1, 2, EdgeType::DependsOn, active()),
            link(2, 1, 3, EdgeType::DependsOn, complete()),
            link(
                3,
                1,
                4,
                EdgeType::DependsOn,
                Some(end(Article, Draft, None)),
            ),
            // Another link type to something unfinished is no blocker.
            link(4, 1, 5, EdgeType::References, active()),
            link(5, 1, 6, EdgeType::RelatedTo, active()),
            // An incoming one: node 7 waits for this node, not the
            // other way round.
            link(6, 7, 1, EdgeType::DependsOn, active()),
            // An answer cached before the standing was sent: unknown,
            // so nothing is said.
            link(7, 1, 8, EdgeType::DependsOn, None),
        ];
        assert_eq!(direct(id(1), &edges), 2);
        assert_eq!(direct(id(7), &edges), 1);
        assert_eq!(direct(id(2), &edges), 0);
        assert_eq!(direct(id(1), &[]), 0);
    }

    fn hop(from: u32, to: u32) -> Hop<u32, String> {
        Hop {
            from,
            to,
            item: format!("item {to:02}"),
        }
    }

    /// (id, depth, via) of a chain, in order.
    fn shape(start: u32, hops: Vec<Hop<u32, String>>) -> Vec<(u32, u32, u32)> {
        chain(start, hops, String::clone)
            .into_iter()
            .map(|c| (c.id, c.depth, c.via))
            .collect()
    }

    #[test]
    fn a_chain_lists_what_lies_beyond_the_direct_dependencies() {
        // 1 → 2 → 3 → 4, and 1 → 5.
        let line = vec![hop(1, 2), hop(2, 3), hop(3, 4), hop(1, 5)];
        assert_eq!(shape(1, line), vec![(3, 2, 2), (4, 3, 2)]);
        // Only direct dependencies: nothing "also".
        assert!(shape(1, vec![hop(1, 2), hop(1, 3)]).is_empty());
        assert!(shape(1, Vec::new()).is_empty());
    }

    #[test]
    fn a_diamond_lists_the_shared_node_once_under_the_first_dependency() {
        // 1 → 2 → 4 and 1 → 3 → 4.
        let diamond = vec![hop(1, 3), hop(3, 4), hop(1, 2), hop(2, 4)];
        assert_eq!(shape(1, diamond.clone()), vec![(4, 2, 2)]);
        // The order of the input changes nothing.
        let mut reversed = diamond;
        reversed.reverse();
        assert_eq!(shape(1, reversed), vec![(4, 2, 2)]);
    }

    #[test]
    fn a_direct_dependency_is_not_listed_again_and_the_start_never_is() {
        // 1 → 2, 1 → 3, and 2 → 3: 3 is the node's own dependency.
        assert!(shape(1, vec![hop(1, 2), hop(1, 3), hop(2, 3)]).is_empty());
        // A loop back to the start (older data can hold one) ends.
        let looped = vec![hop(1, 2), hop(2, 3), hop(3, 1), hop(3, 2)];
        assert_eq!(shape(1, looped), vec![(3, 2, 2)]);
        // A node waiting for itself.
        assert!(shape(1, vec![hop(1, 1)]).is_empty());
    }

    #[test]
    fn links_that_do_not_hang_on_the_start_are_ignored() {
        // 8 → 9 is not reachable from 1 (a walk cut at its limit can
        // leave such a link behind).
        assert_eq!(
            shape(1, vec![hop(1, 2), hop(2, 3), hop(8, 9)]),
            vec![(3, 2, 2)]
        );
    }

    #[test]
    fn the_shortest_way_wins_and_ties_go_by_title_then_id() {
        // 4 is two links away through 2, three through 3 → 5.
        let hops = vec![hop(1, 2), hop(1, 3), hop(3, 5), hop(5, 4), hop(2, 4)];
        assert_eq!(shape(1, hops), vec![(4, 2, 2), (5, 2, 3)]);
        // Same title: the lower id leads.
        let same = |from, to| Hop {
            from,
            to,
            item: "same".to_string(),
        };
        let tie = vec![same(1, 3), same(1, 2), same(3, 4), same(2, 4)];
        assert_eq!(shape(1, tie), vec![(4, 2, 2)]);
    }

    #[test]
    fn a_nodes_blockers_name_the_dependency_they_come_through() {
        let row = |from: u128, to: u128, title: &str| NodeHop {
            from: id(from),
            to: id(to),
            title: title.to_string(),
            node: end(Project, Draft, Some(Active)),
        };
        let answer = node_blockers(
            id(1),
            vec![
                row(1, 2, "Beta"),
                row(2, 3, "gamma"),
                row(3, 4, "Delta"),
                row(1, 5, "alpha"),
                row(5, 3, "gamma"),
            ],
            false,
        );
        // "gamma" is reached through both "alpha" and "Beta": listed
        // once, under "alpha" (titles sort without regard to case).
        let listed: Vec<(&str, u32, &str)> = answer
            .also
            .iter()
            .map(|b| (b.title.as_str(), b.depth, b.via_title.as_str()))
            .collect();
        assert_eq!(listed, vec![("gamma", 2, "alpha"), ("Delta", 3, "alpha")]);
        assert_eq!(answer.also[0].via, id(5));
        assert!(!answer.truncated);
        assert!(node_blockers(id(1), Vec::new(), true).truncated);
    }

    #[test]
    fn an_answer_from_before_the_field_still_reads() {
        let old: NodeBlockers = serde_json::from_str(r#"{"also":[]}"#).expect("parses");
        assert!(!old.truncated);
    }
}
