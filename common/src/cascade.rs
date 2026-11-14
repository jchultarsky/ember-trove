//! The cascade (ONTOLOGY §4.1; slices S6b and S6c of #204, #326).
//!
//! Declaring a master Project complete or cancelled closes what is under
//! it, down the `contains` tree: sub-Projects take the master's terminal
//! state, and open or Waiting Tasks become Cancelled (under completion
//! too: a task closed this way was not done, and gets no completion date).
//!
//! This module is the rule and nothing else: [`plan`] turns the subtree's
//! rows into what a cascade would close. The API reads the rows, shows the
//! plan for confirmation, and applies exactly a plan it has recomputed
//! under its locks (`NodeRepo::set_project_state`). The same function
//! makes the preview and the write, so the two can't disagree.
//!
//! What the plan obeys:
//! - **The actor's own rights only** (§14). A sub-Project closes, and a
//!   project's tasks are cancelled, only where the actor holds an owner or
//!   editor grant of their own. The admin role extends nothing: the
//!   caller computes `editable` without it.
//! - **A child the actor can't edit is skipped**, and so is everything
//!   under it. An open one stays open, so its own children's master is
//!   not closed; it is named, if the actor can view it. One that is
//!   already closed is not named (nothing about it "stays open"), and is
//!   not walked through either: a walk through a hidden one would show
//!   the actor where it sits, and the rule is the same for both. A child
//!   they can't view is not in the plan at all, not even as a count.
//! - **A child that is already closed keeps its state** (it was closed on
//!   its own, perhaps differently), and the walk goes on through it: what
//!   is still open under it is closed. That is the catch-up for projects
//!   closed before the cascade existed.
//! - **State only.** Nothing is deleted, archived or hidden.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, TaskId};
use crate::project_state::ProjectState;

/// One Project of the subtree, as the caller read it for one actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIn {
    pub id: NodeId,
    pub title: String,
    pub state: ProjectState,
    /// The actor holds an owner or editor grant of their own on it.
    pub editable: bool,
    /// The actor may read it (their own node, or any grant).
    pub viewable: bool,
}

/// One task that is still to do (open or waiting) on a node of the
/// subtree. The caller reads only those; done and cancelled tasks are no
/// concern of a cascade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIn {
    pub id: TaskId,
    pub node: NodeId,
    pub title: String,
    /// It belongs to someone other than the actor. It is cancelled all the
    /// same (a task follows its node's rights, §14); the confirmation says
    /// how many there are.
    pub others: bool,
}

/// A sub-Project the cascade closes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CascadeProject {
    pub id: NodeId,
    pub title: String,
    /// The state it leaves: active or parked.
    pub from: ProjectState,
}

/// A task the cascade cancels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CascadeTask {
    pub id: TaskId,
    pub title: String,
    /// Someone else's task.
    pub others: bool,
}

/// The tasks the cascade cancels on one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CascadeTasks {
    pub node_id: NodeId,
    pub node_title: String,
    pub tasks: Vec<CascadeTask>,
}

/// A sub-Project that stays open because the actor may not edit it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CascadeSkipped {
    pub id: NodeId,
    pub title: String,
}

/// What declaring `state` on a master closes, for one actor: the
/// confirmation's content, and what the write applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CascadePlan {
    /// The terminal state being declared.
    pub state: ProjectState,
    /// Whether the actor holds a grant of their own on the master. An
    /// admin without one may declare its state (the admin role's reach),
    /// and the cascade then closes only what they do hold a grant on.
    pub own_grant: bool,
    /// The sub-Projects that close, outermost first.
    pub projects: Vec<CascadeProject>,
    /// The tasks that are cancelled, by project, the master's first.
    pub tasks: Vec<CascadeTasks>,
    /// The sub-Projects that stay open, which the actor can view.
    pub skipped: Vec<CascadeSkipped>,
    /// Stands for exactly this plan. The write takes it back as its
    /// confirmation and refuses when what it would close has changed.
    /// Set by the API ([`CascadePlan::fingerprint`], hashed).
    #[serde(default)]
    pub token: String,
}

impl CascadePlan {
    /// Whether the cascade would change anything. This alone decides
    /// whether a confirmation is needed and whether a closed project is
    /// said to have something open under it: a skipped child stays open
    /// by design, and must not ask to be confirmed forever.
    #[must_use]
    pub fn closes_something(&self) -> bool {
        !self.projects.is_empty() || !self.tasks.is_empty()
    }

    /// How many tasks are cancelled.
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.iter().map(|group| group.tasks.len()).sum()
    }

    /// How many of them belong to someone other than the actor.
    #[must_use]
    pub fn others_task_count(&self) -> usize {
        self.tasks
            .iter()
            .flat_map(|group| &group.tasks)
            .filter(|task| task.others)
            .count()
    }

    /// Every project whose tasks are cancelled or whose state changes:
    /// where the write records the cascade.
    #[must_use]
    pub fn touched_nodes(&self) -> BTreeSet<NodeId> {
        self.projects
            .iter()
            .map(|p| p.id)
            .chain(self.tasks.iter().map(|group| group.node_id))
            .collect()
    }

    /// The plan as one canonical text: the master, the target state and
    /// the ids of everything it shows, each list sorted. Two plans read
    /// alike exactly when they would close the same things for the same
    /// declaration, whatever order the rows came in. The API hashes it
    /// into [`CascadePlan::token`] (the hash lives there: `common` is
    /// also the browser's code and carries no hashing crate).
    ///
    /// The target state is part of it, so a confirmation of "complete"
    /// can't be spent on "cancelled".
    #[must_use]
    pub fn fingerprint(&self, master: NodeId) -> String {
        let mut projects: Vec<String> = self
            .projects
            .iter()
            .map(|p| format!("p:{}:{}", p.id, p.from.as_str()))
            .collect();
        let mut tasks: Vec<String> = self
            .tasks
            .iter()
            .flat_map(|group| &group.tasks)
            .map(|task| format!("t:{}", task.id))
            .collect();
        let mut skipped: Vec<String> = self.skipped.iter().map(|s| format!("s:{}", s.id)).collect();
        projects.sort();
        tasks.sort();
        skipped.sort();
        let mut lines = vec![
            format!("master:{master}"),
            format!("target:{}", self.state.as_str()),
        ];
        lines.extend(projects);
        lines.extend(tasks);
        lines.extend(skipped);
        lines.join("\n")
    }
}

/// What declaring `target` on `master` closes.
///
/// - `nodes`: the Projects reachable from the master down `contains`
///   edges, the master itself excluded or included (it is ignored). Nodes
///   of another kind are not passed: a Project holds only sub-Projects
///   (§3.3), and older data that says otherwise is Data health's.
/// - `edges`: the `contains` edges among them, as (home, child).
/// - `tasks`: the open and waiting tasks on the master and on `nodes`.
///
/// The walk is breadth-first with children in title order, and remembers
/// where it has been: a loop left in older data ends, and no project is
/// planned twice or as its own descendant.
#[must_use]
pub fn plan(
    master: &NodeIn,
    target: ProjectState,
    nodes: &[NodeIn],
    edges: &[(NodeId, NodeId)],
    tasks: &[TaskIn],
) -> CascadePlan {
    let by_id: BTreeMap<NodeId, &NodeIn> = nodes.iter().map(|n| (n.id, n)).collect();
    let mut children: BTreeMap<NodeId, Vec<&NodeIn>> = BTreeMap::new();
    for (home, child) in edges {
        if let Some(node) = by_id.get(child) {
            children.entry(*home).or_default().push(node);
        }
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| (a.title.as_str(), a.id).cmp(&(b.title.as_str(), b.id)));
        list.dedup_by_key(|n| n.id);
    }

    let mut projects = Vec::new();
    let mut skipped = Vec::new();
    // The projects whose tasks are cancelled, in walk order: the master
    // (with the actor's own grant), then each one walked into.
    let mut cancels_tasks_on: Vec<&NodeIn> = Vec::new();
    if master.editable {
        cancels_tasks_on.push(master);
    }
    let mut seen: BTreeSet<NodeId> = BTreeSet::from([master.id]);
    let mut queue: VecDeque<NodeId> = VecDeque::from([master.id]);
    while let Some(home) = queue.pop_front() {
        for child in children.get(&home).map_or(&[][..], Vec::as_slice) {
            if !seen.insert(child.id) {
                continue;
            }
            if !child.editable {
                // Not the actor's to close, and not walked into: what is
                // under it is its own people's to close. Listed as
                // staying open only when it is open, and only to someone
                // who can see it; never counted otherwise.
                if child.viewable && !child.state.is_terminal() {
                    skipped.push(CascadeSkipped {
                        id: child.id,
                        title: child.title.clone(),
                    });
                }
                continue;
            }
            if !child.state.is_terminal() {
                projects.push(CascadeProject {
                    id: child.id,
                    title: child.title.clone(),
                    from: child.state,
                });
            }
            cancels_tasks_on.push(child);
            queue.push_back(child.id);
        }
    }

    let groups = cancels_tasks_on
        .into_iter()
        .filter_map(|node| {
            let on_it: Vec<CascadeTask> = tasks
                .iter()
                .filter(|task| task.node == node.id)
                .map(|task| CascadeTask {
                    id: task.id,
                    title: task.title.clone(),
                    others: task.others,
                })
                .collect();
            (!on_it.is_empty()).then(|| CascadeTasks {
                node_id: node.id,
                node_title: node.title.clone(),
                tasks: on_it,
            })
        })
        .collect();

    CascadePlan {
        state: target,
        own_grant: master.editable,
        projects,
        tasks: groups,
        skipped,
        token: String::new(),
    }
}

// ── The History record ──────────────────────────────────────────────────────

/// At most this many cancelled tasks are sent with one History entry; the
/// entry also says how many there were. The stored record keeps them all.
pub const SHOWN_TASKS: usize = 50;

/// What a `cascade_closed` History entry stores for one project (§4.1:
/// "exactly which sub-Projects and Tasks it closed"):
/// - `state`: the terminal state that was declared;
/// - `root`: the master it came from, on a sub-Project's entry only;
/// - `sub_projects`: on the master's entry, the ids of every project the
///   cascade closed;
/// - `tasks`: this project's own cancelled tasks, as id and title (the
///   title, so the record still reads after a task is deleted).
///
/// Ids, not titles, for other nodes: History is read by everyone who can
/// view its node, and a sub-Project (or the master) may be hidden from
/// them. [`presented`] puts in the titles a reader may see. Nothing
/// personal is nested here: no task owner, no name (the account purge
/// rewrites top-level keys only).
#[must_use]
pub fn recorded(
    state: ProjectState,
    root: Option<NodeId>,
    sub_projects: &[NodeId],
    tasks: &[CascadeTask],
) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "state": state.as_str(),
        "tasks": tasks
            .iter()
            .map(|t| serde_json::json!({ "id": t.id, "title": t.title }))
            .collect::<Vec<_>>(),
    });
    if let Some(obj) = entry.as_object_mut() {
        if let Some(root) = root {
            obj.insert("root".into(), serde_json::json!(root));
        }
        if !sub_projects.is_empty() {
            obj.insert("sub_projects".into(), serde_json::json!(sub_projects));
        }
    }
    entry
}

/// The nodes a stored entry refers to by id: what [`presented`] needs the
/// reader's view of.
#[must_use]
pub fn referenced_nodes(metadata: &serde_json::Value) -> Vec<uuid::Uuid> {
    let as_id = |v: &serde_json::Value| v.as_str().and_then(|s| s.parse::<uuid::Uuid>().ok());
    metadata
        .get("root")
        .and_then(as_id)
        .into_iter()
        .chain(
            metadata
                .get("sub_projects")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(as_id),
        )
        .collect()
}

/// A stored entry as one reader may see it. `titles` holds the nodes
/// **that reader can view**, of those [`referenced_nodes`] named:
/// - `sub_projects` becomes `[{id, title}]` for those, and the others are
///   left out without a trace (§14: never a count of hidden items). When
///   none is left the key goes too: an empty list would say there were
///   some;
/// - `root` becomes `{id, title}`, or goes: a reader who can't view the
///   master is not told there is one;
/// - `tasks` is cut to [`SHOWN_TASKS`], with `tasks_total` beside it.
///
/// Every other key (the actor's name, added by the writer) is kept.
///
/// `None` when that leaves nothing to show (no master, no sub-Project, no
/// task): the entry is then not shown to this reader at all. On its own
/// it would say only that something they can't see was closed.
#[must_use]
pub fn presented(
    metadata: &serde_json::Value,
    titles: &BTreeMap<uuid::Uuid, String>,
) -> Option<serde_json::Value> {
    let mut entry = metadata.clone();
    let obj = entry.as_object_mut()?;
    let named = |v: &serde_json::Value| {
        let id = v.as_str()?.parse::<uuid::Uuid>().ok()?;
        let title = titles.get(&id)?;
        Some(serde_json::json!({ "id": id, "title": title }))
    };
    let root = obj.get("root").and_then(named);
    let has_root = root.is_some();
    match root {
        Some(root) => obj.insert("root".into(), root),
        None => obj.remove("root"),
    };
    let seen: Vec<serde_json::Value> = obj
        .get("sub_projects")
        .and_then(|v| v.as_array())
        .map(|ids| ids.iter().filter_map(named).collect())
        .unwrap_or_default();
    let has_projects = !seen.is_empty();
    if has_projects {
        obj.insert("sub_projects".into(), serde_json::Value::Array(seen));
    } else {
        obj.remove("sub_projects");
    }
    let mut total = 0;
    if let Some(tasks) = obj.get_mut("tasks").and_then(|v| v.as_array_mut()) {
        total = tasks.len();
        tasks.truncate(SHOWN_TASKS);
    }
    obj.insert("tasks_total".into(), serde_json::json!(total));
    (has_root || has_projects || total > 0).then_some(entry)
}

/// One line for a presented entry: "2 sub-projects, 5 tasks". Counts what
/// the reader was given, so a hidden sub-Project is not in the number.
/// `None` when the entry shows nothing (every sub-Project hidden, no
/// tasks).
#[must_use]
pub fn summary(presented: &serde_json::Value) -> Option<String> {
    let count = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let projects = presented
        .get("sub_projects")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);
    let tasks = presented
        .get("tasks_total")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| {
            presented
                .get("tasks")
                .and_then(|v| v.as_array())
                .map(Vec::len)
        })
        .unwrap_or(0);
    let parts: Vec<String> = [
        count(projects, "sub-project", "sub-projects"),
        count(tasks, "task", "tasks"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Why a sub-Project may not be re-opened or made active: its master is
/// closed (§4.1, "no active child under a closed master"; parked is not
/// closed either, owner decision 2026-11-14). The API's 422 text. It names no
/// node, and is not given at all for a master its reader can't view.
pub const MASTER_IS_CLOSED: &str =
    "its master project is closed: make that one active first, then this one";

/// Whether changing a sub-Project from `from` to `to` is refused because
/// its master is closed: it may not **become active**, and it may not
/// **leave a closed state** (so not to parked either). What stays allowed:
/// a repeat of the state it has, a move between complete and cancelled,
/// closing it, and parking one that is still active (a project moved
/// under a closed master is open, by owner decision; setting it aside
/// re-opens nothing). `master_closed` is whether its home is a Project in
/// a terminal state that the actor can view.
#[must_use]
pub fn reopen_refused(from: ProjectState, to: ProjectState, master_closed: bool) -> bool {
    if !master_closed || from == to {
        return false;
    }
    match to {
        ProjectState::Active => true,
        ProjectState::Parked => from.is_terminal(),
        ProjectState::Complete | ProjectState::Cancelled => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProjectState::{Active, Cancelled, Complete, Parked};
    use uuid::Uuid;

    fn nid(n: u128) -> NodeId {
        NodeId(Uuid::from_u128(n))
    }

    fn tid(n: u128) -> TaskId {
        TaskId(Uuid::from_u128(n))
    }

    /// A project the actor owns.
    fn mine(n: u128, title: &str, state: ProjectState) -> NodeIn {
        NodeIn {
            id: nid(n),
            title: title.to_string(),
            state,
            editable: true,
            viewable: true,
        }
    }

    fn task(n: u128, node: u128, title: &str) -> TaskIn {
        TaskIn {
            id: tid(n),
            node: nid(node),
            title: title.to_string(),
            others: false,
        }
    }

    fn closed_ids(plan: &CascadePlan) -> Vec<NodeId> {
        plan.projects.iter().map(|p| p.id).collect()
    }

    fn cancelled_ids(plan: &CascadePlan) -> Vec<TaskId> {
        plan.tasks
            .iter()
            .flat_map(|g| g.tasks.iter().map(|t| t.id))
            .collect()
    }

    #[test]
    fn a_project_with_nothing_under_it_closes_nothing_else() {
        let master = mine(1, "m", Active);
        let p = plan(&master, Complete, &[], &[], &[]);
        assert!(!p.closes_something());
        assert_eq!(p.state, Complete);
        assert!(p.own_grant);
        assert_eq!((p.task_count(), p.others_task_count()), (0, 0));
        assert!(p.touched_nodes().is_empty());
    }

    #[test]
    fn its_own_open_tasks_are_cancelled() {
        let master = mine(1, "m", Active);
        let tasks = [task(10, 1, "a"), task(11, 1, "b")];
        let p = plan(&master, Complete, &[], &[], &tasks);
        assert!(p.closes_something());
        assert!(p.projects.is_empty());
        assert_eq!(cancelled_ids(&p), [tid(10), tid(11)]);
        assert_eq!(p.tasks[0].node_id, nid(1));
        assert_eq!(p.tasks[0].node_title, "m");
        assert_eq!(p.touched_nodes(), BTreeSet::from([nid(1)]));
    }

    #[test]
    fn a_sub_project_takes_the_state_and_its_tasks_go_too() {
        let master = mine(1, "m", Active);
        let nodes = [mine(2, "child", Active), mine(3, "grandchild", Parked)];
        let edges = [(nid(1), nid(2)), (nid(2), nid(3))];
        let tasks = [
            task(10, 1, "on m"),
            task(11, 2, "on child"),
            task(12, 3, "deep"),
        ];
        for target in [Complete, Cancelled] {
            let p = plan(&master, target, &nodes, &edges, &tasks);
            assert_eq!(p.state, target);
            // Outermost first, and a parked project is not closed: it closes.
            assert_eq!(closed_ids(&p), [nid(2), nid(3)]);
            assert_eq!(p.projects[0].from, Active);
            assert_eq!(p.projects[1].from, Parked);
            // The master's tasks first, then each project's in walk order.
            assert_eq!(cancelled_ids(&p), [tid(10), tid(11), tid(12)]);
            assert_eq!(p.touched_nodes(), BTreeSet::from([nid(1), nid(2), nid(3)]));
        }
    }

    #[test]
    fn a_child_already_closed_keeps_its_state_and_is_walked_into() {
        // Cancelled on its own, before its master completes: it is not
        // re-declared. What is still open under it is the catch-up.
        let master = mine(1, "m", Active);
        let nodes = [mine(2, "dropped", Cancelled), mine(3, "under it", Active)];
        let edges = [(nid(1), nid(2)), (nid(2), nid(3))];
        let tasks = [task(11, 2, "left open"), task(12, 3, "deep")];
        let p = plan(&master, Complete, &nodes, &edges, &tasks);
        assert_eq!(closed_ids(&p), [nid(3)], "only what was open closes");
        assert_eq!(cancelled_ids(&p), [tid(11), tid(12)]);
        // The closed child is touched (its tasks), though its state stays.
        assert!(p.touched_nodes().contains(&nid(2)));
    }

    #[test]
    fn a_closed_master_with_something_open_under_it_is_the_catch_up() {
        let master = mine(1, "m", Complete);
        let nodes = [mine(2, "still open", Active)];
        let p = plan(
            &master,
            Complete,
            &nodes,
            &[(nid(1), nid(2))],
            &[task(10, 1, "left")],
        );
        assert!(p.closes_something());
        assert_eq!(closed_ids(&p), [nid(2)]);
        assert_eq!(cancelled_ids(&p), [tid(10)]);
    }

    #[test]
    fn a_child_the_actor_cannot_edit_is_skipped_with_all_under_it() {
        let master = mine(1, "m", Active);
        let theirs = NodeIn {
            editable: false,
            ..mine(2, "theirs, shared for viewing", Active)
        };
        // The actor's own project, but under one that stays open: its
        // master is not closed, so it is not either.
        let nodes = [theirs, mine(3, "mine, under theirs", Active)];
        let edges = [(nid(1), nid(2)), (nid(2), nid(3))];
        let tasks = [task(11, 2, "their task"), task(12, 3, "my task under it")];
        let p = plan(&master, Complete, &nodes, &edges, &tasks);
        assert!(!p.closes_something());
        assert_eq!(
            p.skipped,
            [CascadeSkipped {
                id: nid(2),
                title: "theirs, shared for viewing".into()
            }]
        );
        assert!(cancelled_ids(&p).is_empty());
    }

    /// A child the actor can't edit that is already closed is not said to
    /// "stay open", and the walk still stops there.
    #[test]
    fn a_closed_child_the_actor_cannot_edit_is_not_listed_as_staying_open() {
        let master = mine(1, "m", Active);
        let theirs = NodeIn {
            editable: false,
            ..mine(2, "theirs, closed already", Complete)
        };
        let nodes = [theirs, mine(3, "mine, under theirs", Active)];
        let edges = [(nid(1), nid(2)), (nid(2), nid(3))];
        let p = plan(&master, Complete, &nodes, &edges, &[task(12, 3, "t")]);
        assert!(p.skipped.is_empty(), "{:?}", p.skipped);
        assert!(!p.closes_something());
    }

    #[test]
    fn a_hidden_child_is_not_in_the_plan_at_all() {
        let master = mine(1, "m", Active);
        let hidden = NodeIn {
            editable: false,
            viewable: false,
            ..mine(2, "someone's private project", Active)
        };
        let edges = [(nid(1), nid(2))];
        let with = plan(&master, Complete, &[hidden], &edges, &[task(11, 2, "x")]);
        let without = plan(&master, Complete, &[], &[], &[]);
        // Not named, not counted, and it does not move the token.
        assert_eq!(with, without);
        assert_eq!(with.fingerprint(nid(1)), without.fingerprint(nid(1)));
        assert!(with.skipped.is_empty());
    }

    #[test]
    fn without_a_grant_on_the_master_only_what_the_actor_holds_closes() {
        // An admin acting on someone else's project: the admin role opens
        // the route, and the cascade follows their own grants only.
        let master = NodeIn {
            editable: false,
            ..mine(1, "someone's master", Active)
        };
        let nodes = [
            NodeIn {
                editable: false,
                ..mine(2, "someone's child", Active)
            },
            mine(3, "a child the admin does hold", Active),
        ];
        let edges = [(nid(1), nid(2)), (nid(1), nid(3))];
        let tasks = [
            task(10, 1, "on the master"),
            task(11, 2, "on theirs"),
            task(12, 3, "on the admin's"),
        ];
        let p = plan(&master, Cancelled, &nodes, &edges, &tasks);
        assert!(!p.own_grant);
        assert_eq!(closed_ids(&p), [nid(3)]);
        assert_eq!(cancelled_ids(&p), [tid(12)], "not the master's tasks");
        assert_eq!(p.skipped.len(), 1);

        // And with no grant anywhere, nothing at all.
        let none = plan(&master, Cancelled, &nodes[..1], &edges[..1], &tasks);
        assert!(!none.closes_something());
    }

    #[test]
    fn other_peoples_tasks_are_cancelled_and_counted() {
        let master = mine(1, "m", Active);
        let tasks = [
            task(10, 1, "mine"),
            TaskIn {
                others: true,
                ..task(11, 1, "a colleague's")
            },
        ];
        let p = plan(&master, Complete, &[], &[], &tasks);
        assert_eq!(p.task_count(), 2);
        assert_eq!(p.others_task_count(), 1);
    }

    #[test]
    fn a_loop_ends_and_plans_no_project_twice() {
        // 1 → 2 → 3 → 2, and 3 → 1: older data, before loops were refused.
        let master = mine(1, "m", Active);
        let nodes = [
            mine(2, "a", Active),
            mine(3, "b", Active),
            mine(1, "m", Active),
        ];
        let edges = [
            (nid(1), nid(2)),
            (nid(2), nid(3)),
            (nid(3), nid(2)),
            (nid(3), nid(1)),
        ];
        let p = plan(&master, Complete, &nodes, &edges, &[task(10, 1, "x")]);
        assert_eq!(closed_ids(&p), [nid(2), nid(3)], "never the master itself");
        assert_eq!(cancelled_ids(&p), [tid(10)], "its tasks once");
    }

    #[test]
    fn a_child_with_two_homes_is_planned_once() {
        // Older data: 4 has two homes, both in the tree.
        let master = mine(1, "m", Active);
        let nodes = [
            mine(2, "a", Active),
            mine(3, "b", Active),
            mine(4, "shared", Active),
        ];
        let edges = [
            (nid(1), nid(2)),
            (nid(1), nid(3)),
            (nid(2), nid(4)),
            (nid(3), nid(4)),
        ];
        let p = plan(&master, Complete, &nodes, &edges, &[task(14, 4, "x")]);
        assert_eq!(closed_ids(&p), [nid(2), nid(3), nid(4)]);
        assert_eq!(cancelled_ids(&p), [tid(14)]);
    }

    #[test]
    fn an_edge_to_something_that_was_not_read_is_ignored() {
        // A child of another kind, or one that went away: the caller
        // passes Projects only.
        let master = mine(1, "m", Active);
        let p = plan(
            &master,
            Complete,
            &[],
            &[(nid(1), nid(9))],
            &[task(19, 9, "x")],
        );
        assert!(!p.closes_something());
    }

    #[test]
    fn the_fingerprint_names_the_declaration_and_ignores_row_order() {
        let master = mine(1, "m", Active);
        let nodes = [mine(2, "b", Active), mine(3, "a", Parked)];
        let edges = [(nid(1), nid(2)), (nid(1), nid(3))];
        let tasks = [task(10, 1, "x"), task(11, 2, "y"), task(12, 3, "z")];
        let one = plan(&master, Complete, &nodes, &edges, &tasks);

        let mut nodes_back = nodes.to_vec();
        nodes_back.reverse();
        let mut edges_back = edges.to_vec();
        edges_back.reverse();
        let mut tasks_back = tasks.to_vec();
        tasks_back.reverse();
        let two = plan(&master, Complete, &nodes_back, &edges_back, &tasks_back);
        assert_eq!(one.fingerprint(nid(1)), two.fingerprint(nid(1)));

        // A confirmation of "complete" is not one of "cancelled", nor of
        // another master, nor of a plan with one task more or less.
        let cancel = plan(&master, Cancelled, &nodes, &edges, &tasks);
        assert_ne!(one.fingerprint(nid(1)), cancel.fingerprint(nid(1)));
        assert_ne!(one.fingerprint(nid(1)), one.fingerprint(nid(7)));
        let fewer = plan(&master, Complete, &nodes, &edges, &tasks[..2]);
        assert_ne!(one.fingerprint(nid(1)), fewer.fingerprint(nid(1)));
        // Nor of one where a child closes from a different state.
        let mut parked = nodes.to_vec();
        parked[0].state = Parked;
        let other_from = plan(&master, Complete, &parked, &edges, &tasks);
        assert_ne!(one.fingerprint(nid(1)), other_from.fingerprint(nid(1)));
        // A renamed task or project is the same plan.
        let mut renamed = tasks.to_vec();
        renamed[0].title = "x, renamed".into();
        let same = plan(&master, Complete, &nodes, &edges, &renamed);
        assert_eq!(one.fingerprint(nid(1)), same.fingerprint(nid(1)));
    }

    #[test]
    fn a_skipped_child_is_in_the_fingerprint_but_asks_for_no_confirmation() {
        let master = mine(1, "m", Active);
        let theirs = NodeIn {
            editable: false,
            ..mine(2, "theirs", Active)
        };
        let p = plan(&master, Complete, &[theirs], &[(nid(1), nid(2))], &[]);
        assert!(!p.closes_something(), "it stays open by design");
        let none = plan(&master, Complete, &[], &[], &[]);
        assert_ne!(p.fingerprint(nid(1)), none.fingerprint(nid(1)));
    }

    #[test]
    fn only_leaving_a_closed_state_under_a_closed_master_is_refused() {
        for from in ProjectState::ALL {
            for to in ProjectState::ALL {
                // No closed master (none, an Area, or one the actor can't
                // view): nothing is refused.
                assert!(!reopen_refused(*from, *to, false));
                // A repeat is never a change.
                if from == to {
                    assert!(!reopen_refused(*from, *to, true), "{from:?} again");
                }
                // Closing is always allowed.
                if to.is_terminal() {
                    assert!(!reopen_refused(*from, *to, true), "{from:?} -> {to:?}");
                }
            }
        }
        // It may not leave a closed state: neither active nor parked
        // (owner decision, 2026-11-14).
        for closed in [Complete, Cancelled] {
            assert!(reopen_refused(closed, Active, true));
            assert!(reopen_refused(closed, Parked, true));
        }
        // It may not become active.
        assert!(reopen_refused(Parked, Active, true));
        // An open project under a closed master (moved there) may be set
        // aside, and between the two closed states nothing re-opens.
        assert!(!reopen_refused(Active, Parked, true));
        assert!(!reopen_refused(Complete, Cancelled, true));
        assert!(!reopen_refused(Cancelled, Complete, true));
        assert!(!MASTER_IS_CLOSED.is_empty());
    }

    fn ctask(n: u128, title: &str) -> CascadeTask {
        CascadeTask {
            id: tid(n),
            title: title.to_string(),
            others: false,
        }
    }

    #[test]
    fn the_record_stores_ids_for_nodes_and_titles_for_its_own_tasks() {
        let master = recorded(
            Complete,
            None,
            &[nid(2), nid(3)],
            &[ctask(10, "call the vendor")],
        );
        assert_eq!(master["state"], "complete");
        assert!(master.get("root").is_none(), "the master is its own root");
        assert_eq!(master["sub_projects"].as_array().map(Vec::len), Some(2));
        assert_eq!(master["tasks"][0]["title"], "call the vendor");
        // No title of another node is stored, and nothing about whose
        // task it was.
        let text = master.to_string();
        assert!(!text.contains("others"), "{text}");
        assert_eq!(referenced_nodes(&master), [nid(2).0, nid(3).0]);

        let child = recorded(Complete, Some(nid(1)), &[], &[]);
        assert_eq!(referenced_nodes(&child), [nid(1).0]);
        assert!(child.get("sub_projects").is_none());
    }

    #[test]
    fn a_reader_is_shown_only_the_nodes_they_can_view() {
        let stored = recorded(
            Cancelled,
            Some(nid(1)),
            &[nid(2), nid(3)],
            &[ctask(10, "a"), ctask(11, "b")],
        );
        // This reader can view sub-project 2, and neither 3 nor the root.
        let can_see = BTreeMap::from([(nid(2).0, "visible child".to_string())]);
        let shown = presented(&stored, &can_see).expect("something to show");
        assert!(
            shown.get("root").is_none(),
            "a hidden master is not hinted at"
        );
        assert_eq!(
            shown["sub_projects"],
            serde_json::json!([{ "id": nid(2), "title": "visible child" }])
        );
        assert!(!shown.to_string().contains(&nid(3).to_string()));
        assert!(!shown.to_string().contains(&nid(1).to_string()));
        assert_eq!(shown["tasks_total"], 2);
        assert_eq!(summary(&shown).as_deref(), Some("1 sub-project, 2 tasks"));

        // Someone who can view everything.
        let all = BTreeMap::from([
            (nid(1).0, "the master".to_string()),
            (nid(2).0, "visible child".to_string()),
            (nid(3).0, "other child".to_string()),
        ]);
        let full = presented(&stored, &all).expect("everything to show");
        assert_eq!(full["root"]["title"], "the master");
        assert_eq!(summary(&full).as_deref(), Some("2 sub-projects, 2 tasks"));

        // Someone who can view none of the other nodes is shown the tasks
        // and no sign that anything else closed: not an empty list either.
        let tasks_only = presented(&stored, &BTreeMap::new()).expect("the tasks");
        assert!(tasks_only.get("sub_projects").is_none(), "{tasks_only}");
        assert!(tasks_only.get("root").is_none());
        assert_eq!(summary(&tasks_only).as_deref(), Some("2 tasks"));
    }

    /// An entry that would show its reader nothing is not shown at all:
    /// "closed what was under it", with nothing beside it, would say that
    /// something they can't see exists (§14).
    #[test]
    fn an_entry_with_nothing_for_its_reader_is_left_out() {
        let nobody = BTreeMap::new();
        // The master's entry, every sub-Project hidden, no task of its own.
        let on_master = recorded(Complete, None, &[nid(2)], &[]);
        assert_eq!(presented(&on_master, &nobody), None);
        // A sub-Project's entry, the master hidden, no task of its own.
        let on_child = recorded(Complete, Some(nid(1)), &[], &[]);
        assert_eq!(presented(&on_child, &nobody), None);
        // Each is shown to a reader who can view the other node.
        let sees = BTreeMap::from([(nid(1).0, "m".to_string()), (nid(2).0, "c".to_string())]);
        assert!(presented(&on_master, &sees).is_some());
        assert!(presented(&on_child, &sees).is_some());
    }

    #[test]
    fn a_long_task_list_is_cut_and_still_counted() {
        let many: Vec<CascadeTask> = (0..(SHOWN_TASKS as u128 + 12))
            .map(|n| ctask(100 + n, "t"))
            .collect();
        let stored = recorded(Complete, None, &[], &many);
        assert_eq!(
            stored["tasks"].as_array().map(Vec::len),
            Some(SHOWN_TASKS + 12),
            "the record keeps them all"
        );
        let shown = presented(&stored, &BTreeMap::new()).expect("tasks");
        assert_eq!(shown["tasks"].as_array().map(Vec::len), Some(SHOWN_TASKS));
        assert_eq!(shown["tasks_total"], SHOWN_TASKS + 12);
        assert_eq!(summary(&shown), Some(format!("{} tasks", SHOWN_TASKS + 12)));
    }

    #[test]
    fn the_writers_own_keys_survive_presentation() {
        let mut stored = recorded(Complete, None, &[], &[ctask(10, "a")]);
        stored["actor_name"] = "Alice".into();
        let shown = presented(&stored, &BTreeMap::new()).expect("a task");
        assert_eq!(shown["actor_name"], "Alice");
        assert_eq!(summary(&shown).as_deref(), Some("1 task"));
        // Something that is not a record at all shows nothing.
        let odd = serde_json::json!("not an object");
        assert_eq!(presented(&odd, &BTreeMap::new()), None);
    }

    #[test]
    fn a_plan_round_trips_as_json() {
        let master = mine(1, "m", Active);
        let nodes = [mine(2, "c", Active)];
        let mut p = plan(
            &master,
            Complete,
            &nodes,
            &[(nid(1), nid(2))],
            &[task(10, 1, "x")],
        );
        p.token = "abc".into();
        let back: CascadePlan =
            serde_json::from_value(serde_json::to_value(&p).expect("json")).expect("a plan");
        assert_eq!(back, p);
    }
}
