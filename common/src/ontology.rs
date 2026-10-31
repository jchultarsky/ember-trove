//! The data model's rules as pure functions (`docs/ONTOLOGY.md`, #204).
//!
//! What each node kind may carry (§3.2), which kind may be the home of
//! which (§3.3), and where a graph of edges loops back on itself (§6). The
//! data-health report (#208) uses them to find existing data that breaks the
//! model. The write gates of later slices (S2a, S3a) are to use them too,
//! replacing today's hand-written `== NodeType::Project` checks, so the
//! report and the enforcement can't disagree.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use uuid::Uuid;

use crate::node::NodeType;

/// A feature that only some node kinds carry (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Tasks,
    People,
    /// The open status label and the terminal state (§4.1).
    ProjectStatus,
    /// The Project Plan (§10).
    Plan,
}

impl Capability {
    /// Every capability. `all_lists_every_capability` ties it to the enum.
    pub const ALL: [Self; 4] = [Self::Tasks, Self::People, Self::ProjectStatus, Self::Plan];
}

impl Capability {
    /// Why a write is refused (#216): the API's 422 text, matching the
    /// Data health page's wording for the same rule.
    #[must_use]
    pub fn refusal(self) -> &'static str {
        match self {
            Self::Tasks => "only Projects have tasks",
            Self::People => "only Projects have people",
            Self::ProjectStatus => "only Projects have a project status",
            Self::Plan => "only Projects have a project plan",
        }
    }
}

/// `Ok` when a node of `kind` may carry `capability`, else the refusal
/// text. The API's write gates (#216) call this.
pub fn require(kind: &NodeType, capability: Capability) -> Result<(), &'static str> {
    if allows(kind, capability) {
        Ok(())
    } else {
        Err(capability.refusal())
    }
}

/// Whether a node of `kind` may carry `capability` (§3.2). Every gated
/// capability is Project-only; body, External Links and History (notes
/// included) belong to every kind and aren't gated.
#[must_use]
pub fn allows(kind: &NodeType, capability: Capability) -> bool {
    match capability {
        Capability::Tasks | Capability::People | Capability::ProjectStatus | Capability::Plan => {
            *kind == NodeType::Project
        }
    }
}

/// Whether `parent` may be the `contains` home of `child` (§3.3): an Area
/// holds Areas, Projects and Articles; a Project holds only sub-Projects;
/// Articles and References hold nothing, and a Reference is never held.
#[must_use]
pub fn may_contain(parent: &NodeType, child: &NodeType) -> bool {
    match parent {
        NodeType::Area => matches!(
            child,
            NodeType::Area | NodeType::Project | NodeType::Article
        ),
        NodeType::Project => *child == NodeType::Project,
        NodeType::Article | NodeType::Reference => false,
    }
}

/// Which containment rule, if any, a `parent contains child` edge breaks.
/// Exactly one: a contained Reference is that rule (§6.1), not also a
/// disallowed pair (§3.3), so one bad edge is never reported twice.
#[must_use]
pub fn containment_violation(parent: &NodeType, child: &NodeType) -> Option<ContainmentRule> {
    if *child == NodeType::Reference {
        Some(ContainmentRule::ContainedReference)
    } else if !may_contain(parent, child) {
        Some(ContainmentRule::NotAllowed)
    } else {
        None
    }
}

/// The two ways a `contains` edge can break the containment rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainmentRule {
    /// A Reference is never contained (§6.1).
    ContainedReference,
    /// The pair isn't in the containment table (§3.3).
    NotAllowed,
}

/// A set of nodes that reach each other through edges of one type: a
/// strongly connected component with a cycle in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyclicComponent {
    /// Every node in the component, sorted.
    pub members: Vec<Uuid>,
    /// One cycle through the component, starting at its smallest preferred
    /// member (see [`cyclic_components_preferring`]), else its smallest:
    /// `[a, b, c]` means `a → b → c → a`. A self-loop is `[a]`.
    pub cycle: Vec<Uuid>,
}

/// Every place where `edges` (source → target) loop back on themselves.
///
/// Visibility is the caller's job (§14): pass only edges whose two ends the
/// reader may see, or a hidden node can turn up as a member.
///
/// Each strongly connected component that contains a cycle is reported once,
/// with one representative cycle. Listing every simple cycle instead would
/// grow exponentially on a dense graph. The walk is iterative (Tarjan's
/// algorithm with an explicit stack), so a long chain can't overflow the
/// call stack. The output is deterministic: components are ordered by their
/// smallest member, whatever order the edges came in.
#[must_use]
pub fn cyclic_components(edges: &[(Uuid, Uuid)]) -> Vec<CyclicComponent> {
    cyclic_components_preferring(edges, |_| false)
}

/// [`cyclic_components`], with each representative cycle running through a
/// `prefer`red member when the component has one: the data-health report
/// prefers the reader's own nodes, so the loop it shows is one they can
/// break, not a path through other people's shared nodes.
#[must_use]
pub fn cyclic_components_preferring(
    edges: &[(Uuid, Uuid)],
    prefer: impl Fn(&Uuid) -> bool,
) -> Vec<CyclicComponent> {
    let graph = Graph::new(edges);
    let mut out: Vec<CyclicComponent> = graph
        .strongly_connected()
        .into_iter()
        .filter(|members| members.len() > 1 || graph.has_self_loop(members[0]))
        .map(|members| {
            // Members are sorted, so this is the smallest preferred one.
            let start = members
                .iter()
                .copied()
                .find(|&i| prefer(&graph.ids[i]))
                .unwrap_or(members[0]);
            let cycle = graph.cycle_within(&members, start);
            CyclicComponent {
                members: members.iter().map(|&i| graph.ids[i]).collect(),
                cycle: cycle.iter().map(|&i| graph.ids[i]).collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| a.members.cmp(&b.members));
    out
}

/// The edges as an adjacency list over dense indices. Ids and each node's
/// successors are sorted, which makes every walk below deterministic.
struct Graph {
    ids: Vec<Uuid>,
    adj: Vec<Vec<usize>>,
}

impl Graph {
    fn new(edges: &[(Uuid, Uuid)]) -> Self {
        let ids: Vec<Uuid> = edges
            .iter()
            .flat_map(|&(s, t)| [s, t])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index: BTreeMap<Uuid, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        let mut adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); ids.len()];
        for (s, t) in edges {
            // Both ends are in `index`: it was built from these same edges.
            if let (Some(&s), Some(&t)) = (index.get(s), index.get(t)) {
                adj[s].insert(t);
            }
        }
        Self {
            ids,
            adj: adj
                .into_iter()
                .map(|set| set.into_iter().collect())
                .collect(),
        }
    }

    fn has_self_loop(&self, v: usize) -> bool {
        self.adj[v].binary_search(&v).is_ok()
    }

    /// Tarjan's strongly connected components, iteratively. Each component's
    /// members come back sorted by index (and so by id).
    fn strongly_connected(&self) -> Vec<Vec<usize>> {
        let n = self.ids.len();
        let mut index: Vec<Option<usize>> = vec![None; n];
        let mut low = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut next = 0usize;
        let mut components = Vec::new();
        // (node, position of the next successor to visit)
        let mut call: Vec<(usize, usize)> = Vec::new();

        for root in 0..n {
            if index[root].is_some() {
                continue;
            }
            index[root] = Some(next);
            low[root] = next;
            next += 1;
            stack.push(root);
            on_stack[root] = true;
            call.push((root, 0));

            while let Some(&(v, pos)) = call.last() {
                if let Some(&w) = self.adj[v].get(pos) {
                    if let Some(top) = call.last_mut() {
                        top.1 += 1;
                    }
                    match index[w] {
                        None => {
                            index[w] = Some(next);
                            low[w] = next;
                            next += 1;
                            stack.push(w);
                            on_stack[w] = true;
                            call.push((w, 0));
                        }
                        Some(iw) if on_stack[w] => low[v] = low[v].min(iw),
                        Some(_) => {}
                    }
                    continue;
                }
                // Every successor of `v` is done.
                call.pop();
                if let Some(&(parent, _)) = call.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if index[v] == Some(low[v]) {
                    let mut members = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        members.push(w);
                        if w == v {
                            break;
                        }
                    }
                    members.sort_unstable();
                    components.push(members);
                }
            }
        }
        components
    }

    /// A shortest cycle through `start`, staying inside the component:
    /// breadth-first from it until an edge leads back to it.
    fn cycle_within(&self, members: &[usize], start: usize) -> Vec<usize> {
        if self.has_self_loop(start) {
            return vec![start];
        }
        let inside: BTreeSet<usize> = members.iter().copied().collect();
        let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
        let mut queue = VecDeque::from([start]);
        let mut seen = BTreeSet::from([start]);
        while let Some(v) = queue.pop_front() {
            for &w in &self.adj[v] {
                if w == start {
                    // Walk back from `v` to `start` for the path.
                    let mut path = vec![v];
                    let mut at = v;
                    while let Some(&p) = parent.get(&at) {
                        path.push(p);
                        at = p;
                    }
                    path.reverse();
                    return path;
                }
                if inside.contains(&w) && seen.insert(w) {
                    parent.insert(w, v);
                    queue.push_back(w);
                }
            }
        }
        // Unreachable for a real component (every member reaches every
        // other), but never panic: the members alone still name the loop.
        members.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [NodeType; 4] = [
        NodeType::Article,
        NodeType::Project,
        NodeType::Area,
        NodeType::Reference,
    ];

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    /// The cycle is real: each step, and the step back to the start, is an
    /// edge, and it stays inside the component.
    fn assert_valid_cycle(c: &CyclicComponent, edges: &[(Uuid, Uuid)]) {
        let set: std::collections::HashSet<_> = edges.iter().copied().collect();
        let has = |a: Uuid, b: Uuid| set.contains(&(a, b));
        assert!(!c.cycle.is_empty());
        assert!(
            c.members.contains(&c.cycle[0]),
            "starts inside the component"
        );
        for pair in c.cycle.windows(2) {
            assert!(has(pair[0], pair[1]), "{:?} is not an edge", pair);
        }
        let (last, first) = (c.cycle[c.cycle.len() - 1], c.cycle[0]);
        assert!(has(last, first), "the cycle closes");
        assert!(
            c.members.windows(2).all(|w| w[0] < w[1]),
            "members sorted and unique"
        );
        assert!(c.cycle.iter().all(|n| c.members.binary_search(n).is_ok()));
    }

    #[test]
    fn only_projects_carry_the_gated_capabilities() {
        for kind in &KINDS {
            for cap in Capability::ALL {
                assert_eq!(
                    allows(kind, cap),
                    *kind == NodeType::Project,
                    "{kind:?} / {cap:?}"
                );
            }
        }
    }

    #[test]
    fn the_containment_table_is_exactly_the_ontologys() {
        use NodeType::*;
        let allowed = [
            (Area, Area),
            (Area, Project),
            (Area, Article),
            (Project, Project),
        ];
        for parent in &KINDS {
            for child in &KINDS {
                let expected = allowed.iter().any(|(p, c)| p == parent && c == child);
                assert_eq!(
                    may_contain(parent, child),
                    expected,
                    "{parent:?} contains {child:?}"
                );
            }
        }
    }

    #[test]
    fn require_refuses_with_the_capabilitys_reason() {
        assert_eq!(require(&NodeType::Project, Capability::Tasks), Ok(()));
        assert_eq!(
            require(&NodeType::Article, Capability::Tasks),
            Err("only Projects have tasks")
        );
        for kind in &KINDS {
            for cap in Capability::ALL {
                assert_eq!(require(kind, cap).is_ok(), allows(kind, cap));
                assert!(cap.refusal().starts_with("only Projects have"));
            }
        }
    }

    #[test]
    fn all_lists_every_capability() {
        // Exhaustive on purpose: a new capability fails to compile here
        // until it's given a place in `ALL`.
        fn ordinal(c: Capability) -> usize {
            match c {
                Capability::Tasks => 0,
                Capability::People => 1,
                Capability::ProjectStatus => 2,
                Capability::Plan => 3,
            }
        }
        for (i, c) in Capability::ALL.into_iter().enumerate() {
            assert_eq!(ordinal(c), i);
        }
    }

    #[test]
    fn a_bad_contains_edge_breaks_exactly_one_rule() {
        use NodeType::*;
        for parent in &KINDS {
            for child in &KINDS {
                let got = containment_violation(parent, child);
                let expected = if *child == Reference {
                    Some(ContainmentRule::ContainedReference)
                } else if may_contain(parent, child) {
                    None
                } else {
                    Some(ContainmentRule::NotAllowed)
                };
                assert_eq!(got, expected, "{parent:?} contains {child:?}");
            }
        }
    }

    #[test]
    fn a_reference_is_never_contained() {
        for parent in &KINDS {
            assert!(!may_contain(parent, &NodeType::Reference));
        }
    }

    #[test]
    fn a_chain_or_a_tree_has_no_cycle() {
        let edges = [(id(1), id(2)), (id(2), id(3)), (id(1), id(4))];
        assert!(cyclic_components(&edges).is_empty());
        assert!(cyclic_components(&[]).is_empty());
    }

    #[test]
    fn a_two_node_loop_is_one_component() {
        let edges = [(id(2), id(1)), (id(1), id(2))];
        let found = cyclic_components(&edges);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members, vec![id(1), id(2)]);
        assert_eq!(found[0].cycle, vec![id(1), id(2)]);
        assert_valid_cycle(&found[0], &edges);
    }

    #[test]
    fn a_self_loop_is_a_cycle_of_one() {
        let edges = [(id(5), id(5)), (id(5), id(6))];
        let found = cyclic_components(&edges);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members, vec![id(5)]);
        assert_eq!(found[0].cycle, vec![id(5)]);
    }

    #[test]
    fn disjoint_loops_are_reported_separately_in_order() {
        let edges = [
            (id(20), id(21)),
            (id(21), id(20)),
            (id(1), id(2)),
            (id(2), id(3)),
            (id(3), id(1)),
            (id(3), id(20)), // a bridge doesn't merge them
        ];
        let found = cyclic_components(&edges);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].members, vec![id(1), id(2), id(3)]);
        assert_eq!(found[1].members, vec![id(20), id(21)]);
        for c in &found {
            assert_valid_cycle(c, &edges);
        }
    }

    #[test]
    fn nested_loops_are_one_component_with_a_shortest_cycle() {
        let edges = [
            (id(1), id(2)),
            (id(2), id(3)),
            (id(3), id(1)),
            (id(2), id(1)),
        ];
        let found = cyclic_components(&edges);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members, vec![id(1), id(2), id(3)]);
        assert_eq!(found[0].cycle, vec![id(1), id(2)], "the shortest one");
        assert_valid_cycle(&found[0], &edges);
    }

    #[test]
    fn a_long_chain_closed_into_a_loop_does_not_recurse() {
        // Deep enough that a recursive walk would risk the stack.
        let n = 50_000u128;
        let mut edges: Vec<(Uuid, Uuid)> = (0..n).map(|i| (id(i), id(i + 1))).collect();
        edges.push((id(n), id(0)));
        let found = cyclic_components(&edges);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members.len(), (n + 1) as usize);
        assert_eq!(found[0].cycle.len(), (n + 1) as usize);
        assert_valid_cycle(&found[0], &edges);
    }

    #[test]
    fn a_dense_graph_gives_one_bounded_answer() {
        // Every node points at every other: astronomically many simple
        // cycles, one component, one short representative.
        let n = 40u128;
        let edges: Vec<(Uuid, Uuid)> = (0..n)
            .flat_map(|a| (0..n).filter(move |&b| b != a).map(move |b| (id(a), id(b))))
            .collect();
        let found = cyclic_components(&edges);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].members.len(), n as usize);
        assert_eq!(found[0].cycle.len(), 2);
        assert_valid_cycle(&found[0], &edges);
    }

    #[test]
    fn the_answer_does_not_depend_on_edge_order_or_duplicates() {
        let edges = vec![
            (id(7), id(8)),
            (id(8), id(9)),
            (id(9), id(7)),
            (id(1), id(2)),
            (id(2), id(1)),
            (id(9), id(1)),
        ];
        let mut shuffled = edges.clone();
        shuffled.reverse();
        shuffled.push((id(8), id(9)));
        shuffled.swap(0, 3);
        assert_eq!(cyclic_components(&edges), cyclic_components(&shuffled));
    }

    #[test]
    fn a_preferred_member_gets_the_cycle_through_it() {
        // {1, 2, 3}: 1 ↔ 2 is the short loop from the smallest member, but
        // the reader owns 3, so the loop shown must run through 3.
        let edges = [
            (id(1), id(2)),
            (id(2), id(1)),
            (id(1), id(3)),
            (id(3), id(1)),
        ];
        let plain = cyclic_components(&edges);
        assert_eq!(plain[0].cycle, vec![id(1), id(2)]);
        let mine = cyclic_components_preferring(&edges, |n| *n == id(3));
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].cycle, vec![id(3), id(1)]);
        assert_valid_cycle(&mine[0], &edges);
    }
}
