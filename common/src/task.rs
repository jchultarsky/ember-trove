use chrono::{DateTime, NaiveDate, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
    Cancelled,
}

impl TaskStatus {
    /// `true` for terminal statuses (Done, Cancelled) — the ones a task
    /// checkbox renders as ticked.
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::Cancelled)
    }

    /// The status a done-toggle (checkbox click, My Day `Space`) moves to:
    /// a ticked task reopens, anything else completes. Every task list
    /// shares this rule so a Cancelled task behaves the same everywhere.
    pub fn toggled(&self) -> TaskStatus {
        if self.is_terminal() {
            TaskStatus::Open
        } else {
            TaskStatus::Done
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

/// How often a recurring task should repeat after being marked Done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceRule {
    Daily,
    Weekly,
    Biweekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Task {
    pub id: TaskId,
    /// `None` for standalone (inbox) tasks not yet associated with a node.
    pub node_id: Option<NodeId>,
    pub owner_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    /// Non-null when this task is in the user's My Day for that date.
    pub focus_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    /// Recurrence rule — when Done a new instance is automatically scheduled.
    pub recurrence: Option<RecurrenceRule>,
    /// Manual ordering within My Day (0 = default, higher = later in list).
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Lightweight task summary attached to a project in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskCounts {
    pub open: u32,
    pub in_progress: u32,
    pub done: u32,
    pub cancelled: u32,
}

/// A task enriched with its parent node's title, returned by the My Day endpoint.
/// `node_title` is `None` for standalone (inbox) tasks.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MyDayTask {
    #[serde(flatten)]
    pub task: Task,
    pub node_title: Option<String>,
}

/// Compact task info for display inside dashboard project cards.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskSummary {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub due_date: Option<NaiveDate>,
}

/// One row in the Project Dashboard — a project node plus its task counts.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectDashboardEntry {
    pub node_id: crate::id::NodeId,
    pub title: String,
    pub node_status: String,
    pub task_counts: TaskCounts,
    /// Rendered markdown from the `## Status` heading section, if present.
    pub status_section: Option<String>,
    /// Up to 10 open/in-progress tasks, ordered by priority then due date.
    pub open_tasks: Vec<TaskSummary>,
    /// `true` when more open tasks exist beyond the returned slice.
    pub has_more_tasks: bool,
    /// Most recent activity across the project's own metadata and its tasks.
    /// Equals `MAX(node.updated_at, MAX(tasks.updated_at))`; used to sort
    /// the dashboard so currently-active projects rise to the top.
    pub last_activity_at: DateTime<Utc>,
    /// `true` when the user has pinned this project — pinned projects sort
    /// above non-pinned within their group on the dashboard (v2.9.0).
    #[serde(default)]
    pub pinned: bool,
    /// Optional parent Area (via `edge_type = 'contains'`).  When `Some`,
    /// the dashboard groups this project under the named Area (PARA);
    /// when `None`, the project lands in the "Ungrouped" group (v2.9.0).
    #[serde(default)]
    pub area_id: Option<crate::id::NodeId>,
    #[serde(default)]
    pub area_title: Option<String>,
}

/// Deserialises `Option<Option<T>>` correctly:
/// - field absent        → `None`           (via `#[serde(default)]`)
/// - field present/null  → `Some(None)`
/// - field present/value → `Some(Some(v))`
pub(crate) fn deser_double_opt<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<T>::deserialize(d)?))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateTaskRequest {
    #[garde(length(min = 1, max = 500))]
    pub title: String,
    /// If absent, the task is standalone (no node association).
    #[garde(skip)]
    pub node_id: Option<NodeId>,
    #[garde(skip)]
    pub status: Option<TaskStatus>,
    #[garde(skip)]
    pub priority: Option<TaskPriority>,
    #[garde(skip)]
    pub focus_date: Option<NaiveDate>,
    #[garde(skip)]
    pub due_date: Option<NaiveDate>,
    #[garde(skip)]
    pub recurrence: Option<RecurrenceRule>,
}

/// Every field is optional; `Default` is the no-op PATCH, so callers write
/// `UpdateTaskRequest { status: Some(s), ..Default::default() }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    /// `None` = leave unchanged · `Some(None)` = clear · `Some(Some(d))` = set to date
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    pub focus_date: Option<Option<NaiveDate>>,
    /// `None` = leave unchanged · `Some(None)` = clear · `Some(Some(d))` = set to date
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    pub due_date: Option<Option<NaiveDate>>,
    /// `None` = leave unchanged · `Some(None)` = clear recurrence · `Some(Some(r))` = set
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    pub recurrence: Option<Option<RecurrenceRule>>,
    /// `None` = leave unchanged · `Some(None)` = detach from node · `Some(Some(id))` = associate
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    pub node_id: Option<Option<NodeId>>,
}

/// A done-toggle expressed as PATCHes: the change, and the one that undoes it.
#[derive(Debug, Clone)]
pub struct DoneToggle {
    /// Status the task moves to ([`TaskStatus::toggled`]).
    pub next: TaskStatus,
    pub request: UpdateTaskRequest,
    /// Restores the pre-toggle status (and `focus_date`, if it was stamped).
    pub revert: UpdateTaskRequest,
}

impl DoneToggle {
    /// `true` when the toggle completes the task (offer Undo, strike a spark).
    pub fn completes(&self) -> bool {
        self.next == TaskStatus::Done
    }
}

/// Build the done-toggle for a task with `current` status and `focus_date`,
/// given the viewer's local `today`.
///
/// Completing a task carried over from an earlier day re-stamps `focus_date`
/// to `today` ("done today"): My Day keeps done tasks only when focused on
/// the viewed day, so without the stamp the task would vanish on completion
/// and never count toward "X / Y done". Computed client-side because "today"
/// is the viewer's local date, not the server's UTC date.
pub fn done_toggle(
    current: &TaskStatus,
    focus_date: Option<NaiveDate>,
    today: NaiveDate,
) -> DoneToggle {
    let next = current.toggled();
    let mut request = UpdateTaskRequest {
        status: Some(next.clone()),
        ..Default::default()
    };
    let mut revert = UpdateTaskRequest {
        status: Some(current.clone()),
        ..Default::default()
    };
    if next == TaskStatus::Done
        && let Some(earlier) = focus_date.filter(|d| *d < today)
    {
        request.focus_date = Some(Some(today));
        revert.focus_date = Some(Some(earlier));
    }
    DoneToggle {
        next,
        request,
        revert,
    }
}

/// One entry in a bulk sort-order update (drag-to-reorder in My Day).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReorderTaskEntry {
    pub id: TaskId,
    pub sort_order: i32,
}

/// Request body for `PUT /tasks/reorder`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReorderTasksRequest {
    pub tasks: Vec<ReorderTaskEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_statuses_are_done_and_cancelled() {
        assert!(TaskStatus::Done.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Open.is_terminal());
        assert!(!TaskStatus::InProgress.is_terminal());
    }

    #[test]
    fn toggle_completes_open_and_in_progress() {
        assert_eq!(TaskStatus::Open.toggled(), TaskStatus::Done);
        assert_eq!(TaskStatus::InProgress.toggled(), TaskStatus::Done);
    }

    #[test]
    fn toggle_reopens_terminal_statuses() {
        assert_eq!(TaskStatus::Done.toggled(), TaskStatus::Open);
        // Regression: the node panel and Inbox compared against the string
        // "done", so a (ticked) Cancelled task was re-marked Done instead of
        // reopening.
        assert_eq!(TaskStatus::Cancelled.toggled(), TaskStatus::Open);
    }

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
    }

    #[test]
    fn completing_a_carried_over_task_stamps_today() {
        let t = done_toggle(&TaskStatus::Open, Some(day(9)), day(12));
        assert!(t.completes());
        assert_eq!(t.request.status, Some(TaskStatus::Done));
        assert_eq!(t.request.focus_date, Some(Some(day(12))));
        // Undo restores both the status and the original commitment date.
        assert_eq!(t.revert.status, Some(TaskStatus::Open));
        assert_eq!(t.revert.focus_date, Some(Some(day(9))));
    }

    #[test]
    fn completing_without_carry_over_leaves_focus_alone() {
        for focus in [None, Some(day(12)), Some(day(20))] {
            let t = done_toggle(&TaskStatus::InProgress, focus, day(12));
            assert!(t.completes());
            assert_eq!(t.request.focus_date, None, "focus {focus:?}");
            assert_eq!(t.revert.focus_date, None);
            // Undo returns to In Progress, not a generic Open.
            assert_eq!(t.revert.status, Some(TaskStatus::InProgress));
        }
    }

    #[test]
    fn reopening_never_touches_focus() {
        for from in [TaskStatus::Done, TaskStatus::Cancelled] {
            let t = done_toggle(&from, Some(day(9)), day(12));
            assert!(!t.completes());
            assert_eq!(t.request.status, Some(TaskStatus::Open));
            assert_eq!(t.request.focus_date, None);
        }
    }

    #[test]
    fn default_update_request_serializes_as_noop() {
        let json = serde_json::to_value(UpdateTaskRequest::default()).unwrap();
        // Double-option fields are skipped entirely (absent = leave unchanged);
        // plain options serialize as null, which the API also reads as None.
        assert_eq!(
            json,
            serde_json::json!({ "title": null, "status": null, "priority": null })
        );
        let back: UpdateTaskRequest = serde_json::from_value(json).unwrap();
        assert!(back.focus_date.is_none() && back.due_date.is_none());
    }
}
