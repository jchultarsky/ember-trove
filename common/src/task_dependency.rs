//! A task that waits for another task (S5d; ONTOLOGY §9.3, §6.4).
//!
//! "Task A depends on task B": A cannot proceed or complete until B is
//! Done. It is a hard prerequisite that is **surfaced, never enforced**:
//! a blocked task says so, and nothing stops its person from working on
//! it, ticking it off, or seeing it where it was. A loop of dependencies
//! is a deadlock and is never stored.
//!
//! A link belongs to its two tasks and to nobody's account. What a reader
//! is shown and counted is cut to the tasks on nodes they can view (§14):
//! the API reads through `repo::task_dependency`, which holds that rule.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::id::{NodeId, TaskId};
use crate::ontology::cyclic_components;
use crate::task::TaskStatus;

/// The most tasks one task may wait for: every link counts, shown to the
/// caller or not, so it bounds what is stored and what the loop check
/// walks. Only someone who may edit the task can reach it, and they are
/// told how many of its prerequisites they aren't shown
/// ([`TaskDependencies::not_shown`]), so the limit tells them nothing more.
pub const MAX_PREREQUISITES: usize = 50;

/// How many tasks the picker offers at once.
pub const CANDIDATE_LIMIT: i64 = 8;

/// The other task of a link, as its reader sees it. Only a live task on
/// a node the reader can view is ever sent, so the node's title is theirs
/// to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TaskLink {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub node_id: NodeId,
    pub node_title: String,
}

/// One task's dependencies, for one reader.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TaskDependencies {
    /// The tasks this one waits for.
    pub blocked_by: Vec<TaskLink>,
    /// The tasks that wait for this one. Read-only here: a link is
    /// changed from the task that waits.
    pub blocks: Vec<TaskLink>,
    /// How many more tasks this one waits for that this reader is not
    /// shown: in the trash, or on a node they can't view. Told **only to
    /// someone who may edit the task** (0 for anyone else), never named,
    /// and theirs to remove in one go. Without it such a link would stay
    /// for good, unseen, and still decide which dependencies are refused
    /// as loops (the rule a hidden home got: owner decision, 2026-10-31).
    /// They are not counted as blocking.
    #[serde(default)]
    pub not_shown: u32,
    /// Whether this reader may add and remove prerequisites: the rule the
    /// write routes apply, read for the reader, so a control is offered
    /// only where the call would pass.
    #[serde(default)]
    pub may_edit: bool,
}

impl TaskDependencies {
    /// How many of the prerequisites still hold the task back.
    #[must_use]
    pub fn holding_back(&self) -> u32 {
        let n = self
            .blocked_by
            .iter()
            .filter(|t| holds_back(&t.status))
            .count();
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

/// `POST /tasks/{id}/dependencies`: the task to wait for.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AddDependencyRequest {
    pub depends_on: TaskId,
}

/// A stored link, as a backup holds it: `task_id` waits for
/// `depends_on_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependency {
    pub task_id: TaskId,
    pub depends_on_id: TaskId,
    pub created_at: DateTime<Utc>,
}

/// Why a dependency is refused. No message names a task: the loop one is
/// worked out over links its reader may not see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyRefusal {
    /// A task named as its own prerequisite.
    Itself,
    /// The link would close a loop.
    Loop,
    /// The task already waits for [`MAX_PREREQUISITES`] tasks.
    TooMany,
}

impl DependencyRefusal {
    /// The sentence the API answers with (422).
    #[must_use]
    pub fn message(self) -> String {
        match self {
            DependencyRefusal::Itself => "a task can't wait for itself".to_string(),
            DependencyRefusal::Loop => {
                "this would make a loop of dependencies: the other task already waits for this one"
                    .to_string()
            }
            DependencyRefusal::TooMany => {
                format!("a task can wait for at most {MAX_PREREQUISITES} tasks")
            }
        }
    }
}

/// What the request alone settles, before anything is looked up: a task
/// can't wait for itself.
#[must_use]
pub fn refused_outright(task: TaskId, depends_on: TaskId) -> Option<DependencyRefusal> {
    (task == depends_on).then_some(DependencyRefusal::Itself)
}

/// Whether a prerequisite in this state still holds its dependents back:
/// every state but Done (§6.4: "satisfied when it is Done; never when
/// Cancelled"). A cancelled prerequisite blocks for good; the list shows
/// it as cancelled, and its person removes the link.
#[must_use]
pub fn holds_back(status: &TaskStatus) -> bool {
    *status != TaskStatus::Done
}

/// The states that hold a dependent back, in the database's words. The
/// SQL that counts blockers is built from this list
/// (`repo::task_dependency`), so it can't drift from [`holds_back`].
#[must_use]
pub fn holding_back_states() -> Vec<&'static str> {
    TaskStatus::ALL
        .iter()
        .filter(|s| holds_back(s))
        .map(TaskStatus::as_str)
        .collect()
}

/// Whether a task shows as blocked: it is still to do, and at least one
/// prerequisite its reader can see holds it back. A finished task is not
/// blocked, whatever it once waited for.
#[must_use]
pub fn is_blocked(status: &TaskStatus, blocked_by: u32) -> bool {
    !status.is_terminal() && blocked_by > 0
}

/// The name of the "Blocked" chip: by how many tasks.
#[must_use]
pub fn blocked_label(blocked_by: u32) -> String {
    match blocked_by {
        1 => "Blocked by 1 task".to_string(),
        n => format!("Blocked by {n} tasks"),
    }
}

/// `links` without the ones that make a loop: `(kept, dropped)`, each in
/// the order it came.
///
/// The app never stores a loop (the write is refused), so only an archive
/// edited by hand has one. A restore leaves such links out: a loop is not
/// a storable state (§6.4). Every link inside a group of tasks that reach
/// each other goes, a task waiting for itself included; which single link
/// "closed" the loop can't be known from the archive.
#[must_use]
pub fn without_loops(links: Vec<TaskDependency>) -> (Vec<TaskDependency>, Vec<TaskDependency>) {
    let pairs: Vec<(Uuid, Uuid)> = links
        .iter()
        .map(|l| (l.task_id.0, l.depends_on_id.0))
        .collect();
    let mut group: HashMap<Uuid, usize> = HashMap::new();
    for (n, component) in cyclic_components(&pairs).into_iter().enumerate() {
        for member in component.members {
            group.insert(member, n);
        }
    }
    links.into_iter().partition(|l| {
        match (group.get(&l.task_id.0), group.get(&l.depends_on_id.0)) {
            (Some(a), Some(b)) => a != b,
            _ => true,
        }
    })
}

/// What a restore does with an archive's links.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoredLinks {
    /// The links to insert.
    pub kept: Vec<TaskDependency>,
    /// Links naming a task the restore doesn't bring back, and repeats of
    /// a pair.
    pub without_a_task: usize,
    /// Links left out because they make a loop ([`without_loops`]).
    pub in_a_loop: usize,
}

impl RestoredLinks {
    /// The restore preview's line about what is left out: counts, never a
    /// title or an id. `None` when every link comes back.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        let n = self.without_a_task + self.in_a_loop;
        (n > 0).then(|| {
            format!(
                "{n} task dependenc{} in this backup {} left out: {} in a loop of dependencies, \
                 {} naming a task that is not restored.",
                if n == 1 { "y" } else { "ies" },
                if n == 1 { "is" } else { "are" },
                self.in_a_loop,
                self.without_a_task,
            )
        })
    }
}

/// The links of an archive that a restore inserts, given the tasks it
/// restores (`tasks`): a link needs both its tasks, a pair comes back
/// once, and no loop comes back at all. The preview and the restore both
/// call this, so the preview's count is what the restore does.
#[must_use]
pub fn restorable(links: Vec<TaskDependency>, tasks: &HashSet<TaskId>) -> RestoredLinks {
    let before = links.len();
    let mut seen = HashSet::new();
    let whole: Vec<TaskDependency> = links
        .into_iter()
        .filter(|l| tasks.contains(&l.task_id) && tasks.contains(&l.depends_on_id))
        .filter(|l| seen.insert((l.task_id, l.depends_on_id)))
        .collect();
    let without_a_task = before - whole.len();
    let (kept, looped) = without_loops(whole);
    RestoredLinks {
        kept,
        without_a_task,
        in_a_loop: looped.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> TaskId {
        TaskId(Uuid::from_u128(n))
    }

    fn link(task: u128, depends_on: u128) -> TaskDependency {
        TaskDependency {
            task_id: id(task),
            depends_on_id: id(depends_on),
            created_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    fn pairs(links: &[TaskDependency]) -> Vec<(u128, u128)> {
        let mut out: Vec<(u128, u128)> = links
            .iter()
            .map(|l| (l.task_id.0.as_u128(), l.depends_on_id.0.as_u128()))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn only_a_done_prerequisite_lets_its_dependents_go() {
        for status in TaskStatus::ALL {
            assert_eq!(
                holds_back(status),
                *status != TaskStatus::Done,
                "{status:?}"
            );
        }
        // §6.4: "never when Cancelled".
        assert!(holds_back(&TaskStatus::Cancelled));
        assert_eq!(holding_back_states(), ["open", "waiting", "cancelled"]);
    }

    #[test]
    fn a_task_is_blocked_only_while_it_is_still_to_do() {
        assert!(is_blocked(&TaskStatus::Open, 1));
        assert!(is_blocked(&TaskStatus::Waiting, 3));
        assert!(!is_blocked(&TaskStatus::Open, 0));
        assert!(!is_blocked(&TaskStatus::Done, 2));
        assert!(!is_blocked(&TaskStatus::Cancelled, 2));
    }

    #[test]
    fn the_chip_says_by_how_many_tasks() {
        assert_eq!(blocked_label(1), "Blocked by 1 task");
        assert_eq!(blocked_label(2), "Blocked by 2 tasks");
    }

    #[test]
    fn a_task_cannot_wait_for_itself() {
        assert_eq!(
            refused_outright(id(1), id(1)),
            Some(DependencyRefusal::Itself)
        );
        assert_eq!(refused_outright(id(1), id(2)), None);
    }

    #[test]
    fn no_refusal_names_a_task() {
        for refusal in [
            DependencyRefusal::Itself,
            DependencyRefusal::Loop,
            DependencyRefusal::TooMany,
        ] {
            let message = refusal.message();
            assert!(!message.is_empty());
            assert!(
                !message.contains('-') && !message.contains('“'),
                "{message}"
            );
        }
        assert!(DependencyRefusal::Loop.message().contains("loop"));
        assert!(DependencyRefusal::TooMany.message().contains("50"));
    }

    #[test]
    fn the_holding_back_count_leaves_out_finished_prerequisites() {
        let task = |status| TaskLink {
            id: id(9),
            title: "t".to_string(),
            status,
            node_id: NodeId(Uuid::from_u128(1)),
            node_title: "p".to_string(),
        };
        let deps = TaskDependencies {
            blocked_by: vec![
                task(TaskStatus::Open),
                task(TaskStatus::Done),
                task(TaskStatus::Cancelled),
            ],
            blocks: vec![task(TaskStatus::Open)],
            // Not shown, so not counted as holding the task back.
            not_shown: 4,
            may_edit: true,
        };
        assert_eq!(deps.holding_back(), 2);
        assert_eq!(TaskDependencies::default().holding_back(), 0);
    }

    #[test]
    fn links_without_a_loop_are_all_kept() {
        // A diamond is no loop: 1 → 2 → 4 and 1 → 3 → 4.
        let links = vec![link(1, 2), link(1, 3), link(2, 4), link(3, 4)];
        let (kept, dropped) = without_loops(links.clone());
        assert_eq!(kept, links);
        assert!(dropped.is_empty());
        assert_eq!(without_loops(Vec::new()), (Vec::new(), Vec::new()));
    }

    #[test]
    fn a_loop_is_left_out_and_the_rest_is_kept() {
        // 1 ⇄ 2 is a loop of two; 3 → 4 → 5 → 3 a loop of three; 6 waits
        // for itself. 7 → 1 and 5 → 8 touch a loop without being in one.
        let links = vec![
            link(1, 2),
            link(2, 1),
            link(3, 4),
            link(4, 5),
            link(5, 3),
            link(6, 6),
            link(7, 1),
            link(5, 8),
            link(9, 10),
        ];
        let (kept, dropped) = without_loops(links);
        assert_eq!(pairs(&kept), [(5, 8), (7, 1), (9, 10)]);
        assert_eq!(
            pairs(&dropped),
            [(1, 2), (2, 1), (3, 4), (4, 5), (5, 3), (6, 6)]
        );
        // What is kept has no loop left in it.
        let left: Vec<(Uuid, Uuid)> = kept
            .iter()
            .map(|l| (l.task_id.0, l.depends_on_id.0))
            .collect();
        assert!(cyclic_components(&left).is_empty());
    }

    #[test]
    fn the_order_of_an_archive_does_not_change_what_is_left_out() {
        let mut links = vec![link(1, 2), link(2, 3), link(3, 1), link(3, 4), link(5, 1)];
        let (kept, dropped) = without_loops(links.clone());
        links.reverse();
        let (kept_reversed, dropped_reversed) = without_loops(links);
        assert_eq!(pairs(&kept), pairs(&kept_reversed));
        assert_eq!(pairs(&dropped), pairs(&dropped_reversed));
        assert_eq!(pairs(&kept), [(3, 4), (5, 1)]);
    }

    #[test]
    fn a_link_between_two_separate_loops_is_kept() {
        // 1 ⇄ 2 and 3 ⇄ 4 are two loops; 2 → 3 joins them one way only.
        let links = vec![link(1, 2), link(2, 1), link(3, 4), link(4, 3), link(2, 3)];
        let (kept, dropped) = without_loops(links);
        assert_eq!(pairs(&kept), [(2, 3)]);
        assert_eq!(dropped.len(), 4);
    }

    #[test]
    fn a_restore_keeps_whole_links_once_and_no_loop() {
        let tasks: HashSet<TaskId> = (1..=6).map(id).collect();
        let links = vec![
            link(1, 2),
            link(1, 2), // the pair again
            link(2, 3),
            link(3, 99), // a task the archive doesn't hold
            link(98, 1),
            link(4, 5),
            link(5, 4), // a loop
            link(6, 6), // itself
        ];
        let restored = restorable(links, &tasks);
        assert_eq!(pairs(&restored.kept), [(1, 2), (2, 3)]);
        assert_eq!(restored.without_a_task, 3);
        assert_eq!(restored.in_a_loop, 3);
        let warning = restored.warning().expect("something is left out");
        assert!(warning.starts_with("6 task dependencies"), "{warning}");
        assert!(warning.contains("3 in a loop"), "{warning}");
        assert!(warning.contains("3 naming a task"), "{warning}");

        let all = restorable(vec![link(1, 2)], &tasks);
        assert_eq!(all.kept.len(), 1);
        assert_eq!(all.warning(), None);
        let one = restorable(vec![link(1, 99)], &tasks);
        assert!(
            one.warning()
                .expect("one left out")
                .starts_with("1 task dependency in this backup is left out"),
        );
        assert_eq!(restorable(Vec::new(), &tasks), RestoredLinks::default());

        // A link to a task that is not restored never joins the loop
        // check: 1 → 2 → 99 → 1 is a loop only through a task that isn't
        // coming back, so 1 → 2 is kept.
        let through_nothing = restorable(vec![link(1, 2), link(2, 99), link(99, 1)], &tasks);
        assert_eq!(pairs(&through_nothing.kept), [(1, 2)]);
        assert_eq!(through_nothing.without_a_task, 2);
        assert_eq!(through_nothing.in_a_loop, 0);
    }

    #[test]
    fn the_dependencies_of_a_task_read_without_the_edit_flag() {
        // An answer cached before the flag existed must not offer controls.
        let deps: TaskDependencies =
            serde_json::from_str(r#"{"blocked_by":[],"blocks":[]}"#).expect("parses");
        assert!(!deps.may_edit);
        assert_eq!(deps.not_shown, 0);
    }
}
