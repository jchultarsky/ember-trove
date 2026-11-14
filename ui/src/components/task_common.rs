//! Shared task helpers — pure functions used by `task_panel`, `my_day_view`,
//! Data health, and the reusable `TaskRowInner` component.
//!
//! Centralises status / priority / recurrence parsing, formatting, and
//! comparison so that every task-related view speaks the same language.

use std::sync::Arc;

use common::id::TaskId;
use common::task::{MyDayTask, RecurrenceRule, Task, TaskPriority, TaskStatus};
use leptos::prelude::*;

use crate::components::toast::{ToastLevel, ToastState};

// ── Undo (soft delete) ──────────────────────────────────────────────────────

/// Build the "Undo" action for a deleted task: restores it server-side and
/// bumps `refresh` so the list refetches.
///
/// Owner-independent by design (see `ToastAction`): captures only Copy values
/// (id, app-root signals, `ToastState`), so it stays valid after the deleting
/// row has unmounted.
pub fn undo_restore_task(
    task_id: TaskId,
    refresh: RwSignal<u32>,
    toast_state: Option<ToastState>,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::restore_task(task_id).await {
                Ok(_) => refresh.update(|n| *n += 1),
                Err(e) => {
                    if let Some(ts) = toast_state {
                        ts.push(ToastLevel::Error, format!("Undo failed: {e}"));
                    }
                }
            }
        });
    })
}

// ── Status ──────────────────────────────────────────────────────────────────

/// Where a task opens: on its node's page, where `?task=` scrolls to and
/// highlights it. A task with no Project has no list of its own any more
/// (S4c, #240): it opens Data health, whose `OrphanTask` rule lists it with
/// "Move to a Project".
pub fn task_href(task: uuid::Uuid, node: Option<uuid::Uuid>) -> String {
    match node {
        Some(nid) => format!("/nodes/{nid}?task={task}"),
        None => "/data-health".to_string(),
    }
}

/// Returns `true` for terminal statuses (Done, Cancelled).
pub fn status_done(s: &TaskStatus) -> bool {
    s.is_terminal()
}

/// The badge of a task that is on someone or something else (S5a, #314).
/// Cool, not warm: warm colour means urgency here, and a waiting task asks
/// nothing of its person for now.
pub const WAITING_TONE: &str = "bg-sky-100 dark:bg-sky-900/30 text-sky-700 dark:text-sky-300";

// ── Priority ────────────────────────────────────────────────────────────────

/// Parse a priority from a form value; anything unknown is Medium.
pub fn parse_priority(s: &str) -> TaskPriority {
    TaskPriority::parse(s).unwrap_or(TaskPriority::Medium)
}

/// Raw hex colour string for a priority level (e.g. `"#dc2626"`).
pub fn priority_color_hex(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::High => "#dc2626",
        TaskPriority::Medium => "#d97706",
        TaskPriority::Low => "#6b7280",
    }
}

// ── Recurrence ──────────────────────────────────────────────────────────────

pub fn recurrence_label(r: &RecurrenceRule) -> &'static str {
    match r {
        RecurrenceRule::Daily => "Daily",
        RecurrenceRule::Weekly => "Weekly",
        RecurrenceRule::Biweekly => "Every 2 weeks",
        RecurrenceRule::Monthly => "Monthly",
        RecurrenceRule::Yearly => "Yearly",
    }
}

// ── Node type ───────────────────────────────────────────────────────────────

// ── My Day predicate ────────────────────────────────────────────────────────

/// Returns `true` when the task is currently visible in today's My Day list.
///
/// Mirrors the server's carry-forward rule in `list_my_day`: a task shows up
/// in My Day when it is focused on `today` exactly, or on an earlier day
/// while it still asks for attention (open, and its project not parked or
/// closed; S6a). `project` is the state of the task's Project. This keeps
/// the node-detail lightbulb in sync with what actually appears in My Day.
pub fn is_in_my_day(
    task: &Task,
    today: chrono::NaiveDate,
    project: Option<common::project_state::ProjectState>,
) -> bool {
    // The rule lives in `common` (#281, 3.5).
    task.in_my_day(today, project)
}

// ── Backlog partitioning ────────────────────────────────────────────────────

/// Split backlog tasks into `(overdue, upcoming)`. Overdue = an open task
/// whose `due_date` is strictly before `today` and whose project is not
/// parked or closed (`Task::in_overdue_zone`); everything else (no due
/// date, due today, due later, already done, waiting on someone else, or
/// on a project that is set aside) stays in the main backlog. Relative
/// order is preserved within each group.
pub fn partition_overdue(
    tasks: Vec<MyDayTask>,
    today: chrono::NaiveDate,
) -> (Vec<MyDayTask>, Vec<MyDayTask>) {
    tasks.into_iter().partition(|t| t.in_overdue_zone(today))
}

// ── Sorting ─────────────────────────────────────────────────────────────────

/// Sort tasks by `sort_order` first, then `created_at` as a tiebreak.
/// Used by TaskPanel (node-scoped task list).
pub fn sort_tasks_by_order(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use common::id::TaskId;
    use common::task::{Task, TaskPriority, TaskStatus};

    fn mk(due: Option<NaiveDate>, status: TaskStatus) -> MyDayTask {
        MyDayTask {
            task: Task {
                id: TaskId(uuid::Uuid::new_v4()),
                node_id: None,
                owner_id: "t".to_string(),
                title: "t".to_string(),
                status,
                priority: TaskPriority::Medium,
                focus_date: None,
                due_date: due,
                recurrence: None,
                sort_order: 0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                completed_at: None,
            },
            node_title: None,
            project_state: None,
        }
    }

    #[test]
    fn a_late_task_of_a_parked_or_closed_project_stays_in_the_backlog() {
        use common::project_state::ProjectState;
        let today = NaiveDate::from_ymd_opt(2026, 6, 10).expect("date");
        let yesterday = NaiveDate::from_ymd_opt(2026, 6, 9);
        let on = |state: Option<ProjectState>| MyDayTask {
            project_state: state,
            ..mk(yesterday, TaskStatus::Open)
        };
        let tasks = vec![
            on(None),
            on(Some(ProjectState::Active)),
            on(Some(ProjectState::Parked)),
            on(Some(ProjectState::Complete)),
            on(Some(ProjectState::Cancelled)),
        ];
        let (overdue, upcoming) = partition_overdue(tasks, today);
        let states = |rows: &[MyDayTask]| rows.iter().map(|t| t.project_state).collect::<Vec<_>>();
        assert_eq!(states(&overdue), [None, Some(ProjectState::Active)]);
        assert_eq!(
            states(&upcoming),
            [
                Some(ProjectState::Parked),
                Some(ProjectState::Complete),
                Some(ProjectState::Cancelled)
            ],
            "not lost: in the backlog, in their order"
        );
    }

    #[test]
    fn overdue_is_open_and_strictly_past_due() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 10).expect("date");
        let yesterday = NaiveDate::from_ymd_opt(2026, 6, 9);
        let tomorrow = NaiveDate::from_ymd_opt(2026, 6, 11);
        let tasks = vec![
            mk(yesterday, TaskStatus::Open),   // overdue
            mk(Some(today), TaskStatus::Open), // due today → upcoming
            mk(tomorrow, TaskStatus::Open),    // future → upcoming
            mk(None, TaskStatus::Open),        // undated → upcoming
            mk(yesterday, TaskStatus::Done),   // done → upcoming (no guilt pile)
            // On someone else: past due, but not put under Overdue (S5a).
            mk(yesterday, TaskStatus::Waiting),
        ];
        let (overdue, upcoming) = partition_overdue(tasks, today);
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].task.due_date, yesterday);
        assert_eq!(overdue[0].task.status, TaskStatus::Open);
        assert_eq!(upcoming.len(), 5);
        // The waiting one is in the backlog, in its place.
        assert_eq!(
            upcoming.last().map(|t| t.task.status.clone()),
            Some(TaskStatus::Waiting)
        );
    }
}
