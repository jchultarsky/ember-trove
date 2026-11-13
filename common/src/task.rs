use chrono::{DateTime, NaiveDate, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, TaskId};
use crate::project_state::{ProjectState, lets_tasks_ask};

/// A Task's lifecycle (ONTOLOGY §9.3): Open, Waiting, Done or Cancelled.
///
/// There was an `in_progress` until S5a (#314); it became Open (owner
/// decision, 2026-10-30). The database keeps it out with a CHECK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Reads `"in_progress"` too: a backup made before S5a still restores,
    /// and a client that sends the old word is not refused. It is never
    /// written, and [`TaskStatus::parse`] (the database's words) rejects it.
    #[serde(alias = "in_progress")]
    Open,
    /// On someone or something else. What it waits on is in the task's text.
    Waiting,
    /// Completed. Carries the completion date ([`Task::completed_at`]).
    Done,
    /// Something prevented it, for good. No completion date.
    Cancelled,
}

wire_enum!(TaskStatus {
    Open => "open",
    Waiting => "waiting",
    Done => "done",
    Cancelled => "cancelled",
});

impl TaskStatus {
    /// The states a person sets by hand, in the task editor's picker
    /// (owner decision, 2026-11-13). Done is not among them: it is the
    /// checkbox, which also stamps the completion date's day in My Day.
    pub const PICKABLE: [Self; 3] = [Self::Open, Self::Waiting, Self::Cancelled];

    /// Whether its person can act on the task now: Open only. A Waiting
    /// task is on someone or something else, so My Day doesn't carry it
    /// over or list it as overdue (owner decision, 2026-11-13); it stays
    /// in the backlog and on its Project.
    #[must_use]
    pub fn is_actionable(&self) -> bool {
        matches!(self, TaskStatus::Open)
    }

    /// The user-facing name, for every badge and picker.
    pub fn label(&self) -> &'static str {
        match self {
            TaskStatus::Open => "Open",
            TaskStatus::Waiting => "Waiting",
            TaskStatus::Done => "Done",
            TaskStatus::Cancelled => "Cancelled",
        }
    }

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

wire_enum!(TaskPriority {
    Low => "low",
    Medium => "medium",
    High => "high",
});

impl TaskPriority {
    /// The user-facing name. `ALL` + `label` is the single option list that
    /// every priority control renders (#132; it used to be written out in
    /// each editor and the add-task form).
    pub fn label(&self) -> &'static str {
        match self {
            TaskPriority::Low => "Low",
            TaskPriority::Medium => "Medium",
            TaskPriority::High => "High",
        }
    }
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

wire_enum!(RecurrenceRule {
    Daily => "daily",
    Weekly => "weekly",
    Biweekly => "biweekly",
    Monthly => "monthly",
    Yearly => "yearly",
});

impl RecurrenceRule {
    /// The next occurrence after `d`. Months and years clamp to the target
    /// month's last day (Jan 31 → Feb 28/29, Feb 29 → Feb 28 next year), the
    /// way calendars do. It used to fall back to the 28th for every short
    /// month, so a series on the 29th–31st drifted to the 28th permanently.
    pub fn advance(&self, d: NaiveDate) -> NaiveDate {
        use chrono::Months;
        match self {
            RecurrenceRule::Daily => d + chrono::Duration::days(1),
            RecurrenceRule::Weekly => d + chrono::Duration::weeks(1),
            RecurrenceRule::Biweekly => d + chrono::Duration::weeks(2),
            RecurrenceRule::Monthly => d.checked_add_months(Months::new(1)).unwrap_or(d),
            RecurrenceRule::Yearly => d.checked_add_months(Months::new(12)).unwrap_or(d),
        }
    }
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
    /// When the task became Done (§9.3); cleared when it is reopened.
    /// `None` for every other state, and for a task completed before S5a:
    /// that date was never recorded, and is not made up.
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

/// Past its due date and still to do. Done and Cancelled are never
/// overdue. The one rule for every badge and list (#281, 3.5: it was
/// written three times in the UI, and one copy read the status only once,
/// so its badge stayed red after the task was ticked).
#[must_use]
pub fn is_overdue(status: &TaskStatus, due: Option<NaiveDate>, today: NaiveDate) -> bool {
    !status.is_terminal() && due.is_some_and(|d| d < today)
}

impl Task {
    /// The completion date a restore writes: the archived one, and only
    /// for a done task. The database refuses a date on any other state
    /// (migration 050), and one such row in a hand-edited archive must not
    /// fail the whole restore.
    #[must_use]
    pub fn restorable_completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
            .filter(|_| self.status == TaskStatus::Done)
    }

    /// See [`is_overdue`].
    #[must_use]
    pub fn is_overdue(&self, today: NaiveDate) -> bool {
        is_overdue(&self.status, self.due_date, today)
    }

    /// Whether the task may ask for its person's attention now: it can be
    /// acted on ([`TaskStatus::is_actionable`], Open only) **and** its
    /// Project is not set aside or closed
    /// (`project_state::lets_tasks_ask`; S6a, owner decision 2026-11-13).
    /// `project` is the state of the task's Project, `None` for a task on
    /// no Project.
    ///
    /// The one rule behind My Day's carry-over and its Overdue zone. Both
    /// take the project's state as a required argument, so no caller can
    /// decide without it: a task that one place counted out of Today and
    /// another out of the Backlog would be in neither.
    #[must_use]
    pub fn asks_for_attention(&self, project: Option<ProjectState>) -> bool {
        self.status.is_actionable() && lets_tasks_ask(project)
    }

    /// Whether the task is in `today`'s My Day, by the server's
    /// carry-forward rule (`list_my_day`): focused on `today` exactly, or
    /// focused on an earlier day and still asking for attention
    /// ([`Task::asks_for_attention`]). A task that is done, cancelled or
    /// waiting doesn't carry over (waiting since S5a), nor does one whose
    /// Project is parked or closed (S6a). A task its person put in today's
    /// list stays there whatever its state or its project's.
    #[must_use]
    pub fn in_my_day(&self, today: NaiveDate, project: Option<ProjectState>) -> bool {
        match self.focus_date {
            Some(d) if d == today => true,
            Some(d) if d < today => self.asks_for_attention(project),
            _ => false,
        }
    }

    /// Whether My Day lists the task under Overdue: past its due date and
    /// asking for attention. A Waiting task past its due date, or one on a
    /// parked project, keeps the red date on its row ([`is_overdue`] is
    /// the fact), but is not put in front of its person as something to do
    /// now.
    #[must_use]
    pub fn in_overdue_zone(&self, today: NaiveDate, project: Option<ProjectState>) -> bool {
        self.asks_for_attention(project) && self.is_overdue(today)
    }
}

/// Lightweight task summary attached to a project in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskCounts {
    pub open: u32,
    pub waiting: u32,
    pub done: u32,
    pub cancelled: u32,
}

/// A task enriched with its parent node's title, returned by the My Day endpoint.
/// `node_title` is `None` for a task with no Project, and for a node the
/// caller can no longer see (#230).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MyDayTask {
    #[serde(flatten)]
    pub task: Task,
    pub node_title: Option<String>,
    /// The declared state of the task's Project (S6a), from the same
    /// visibility-scoped read as `node_title`: `None` for a task on no
    /// Project. A task on a node its reader can't see is not in the feeds
    /// at all, so this says nothing about a hidden node.
    #[serde(default)]
    pub project_state: Option<ProjectState>,
}

impl MyDayTask {
    /// [`Task::in_my_day`], with this task's own project.
    #[must_use]
    pub fn in_my_day(&self, today: NaiveDate) -> bool {
        self.task.in_my_day(today, self.project_state)
    }

    /// [`Task::in_overdue_zone`], with this task's own project.
    #[must_use]
    pub fn in_overdue_zone(&self, today: NaiveDate) -> bool {
        self.task.in_overdue_zone(today, self.project_state)
    }

    /// The state of the task's Project when that state keeps the task
    /// quiet (parked, complete, cancelled): what the Backlog row says
    /// beside the project's name. `None` for an active project or none.
    #[must_use]
    pub fn quiet_project(&self) -> Option<ProjectState> {
        self.project_state.filter(|s| !s.asks_for_attention())
    }
}

/// Where a feed task (`MyDayTask`) lives, for its parent chip (#230).
///
/// The feeds null `node_title` when the task's node is no longer visible to
/// the caller (a revoked share), while `node_id` stays: the task is still
/// theirs. That case must not read as "No Project".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskParent {
    /// A task with no Project (Data health lists it; S4c, #240).
    NoProject,
    /// A task on a node the caller can see, with its title.
    Node(String),
    /// A task on a node the caller can no longer see.
    Unavailable,
}

impl TaskParent {
    pub fn of(node_id: Option<NodeId>, node_title: Option<String>) -> Self {
        match (node_id, node_title) {
            (None, _) => Self::NoProject,
            (Some(_), Some(title)) => Self::Node(title),
            (Some(_), None) => Self::Unavailable,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::NoProject => "No Project",
            Self::Node(title) => title,
            Self::Unavailable => "Node unavailable",
        }
    }

    /// Material Symbols glyph for the chip.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::NoProject => "link_off",
            Self::Node(_) => "rocket_launch",
            Self::Unavailable => "lock",
        }
    }

    /// Whether a click can open the task's home. The node page of an
    /// unavailable node only answers with an error.
    pub fn is_reachable(&self) -> bool {
        !matches!(self, Self::Unavailable)
    }
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
    /// The project's status token (#143).
    #[serde(default)]
    pub project_status: Option<String>,
    /// The project's declared state (S6a). The dashboard's Area groups
    /// show active projects and its Parked group the parked ones; a
    /// complete or cancelled project is not listed at all.
    #[serde(default)]
    pub project_state: Option<crate::project_state::ProjectState>,
    /// When that state was last declared, if anyone has
    /// (`Node::project_state_at`): "Parked since Nov 13".
    #[serde(default)]
    pub project_state_at: Option<DateTime<Utc>>,
    /// Up to 10 tasks still to do: open ones first, then waiting ones, each
    /// by priority then due date.
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
    /// The project's current Brief (#282), if it has one: the narrative.
    /// It replaced the `## Status` section the dashboard used to read out
    /// of the body (retired 2026-11-12).
    #[serde(default)]
    pub brief: Option<crate::brief::ProjectBrief>,
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
    #[garde(length(min = 1, max = 500), custom(crate::node::not_blank))]
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
///
/// Validated like [`CreateTaskRequest`] (#281, 1.5): an empty title used to
/// reach the database's CHECK and come back as a 500.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateTaskRequest {
    #[garde(length(min = 1, max = 500), custom(crate::node::not_blank_if_set))]
    pub title: Option<String>,
    #[garde(skip)]
    pub status: Option<TaskStatus>,
    #[garde(skip)]
    pub priority: Option<TaskPriority>,
    /// `None` = leave unchanged · `Some(None)` = clear · `Some(Some(d))` = set to date
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    #[garde(skip)]
    pub focus_date: Option<Option<NaiveDate>>,
    /// `None` = leave unchanged · `Some(None)` = clear · `Some(Some(d))` = set to date
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    #[garde(skip)]
    pub due_date: Option<Option<NaiveDate>>,
    /// `None` = leave unchanged · `Some(None)` = clear recurrence · `Some(Some(r))` = set
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    #[garde(skip)]
    pub recurrence: Option<Option<RecurrenceRule>>,
    /// `None` = leave unchanged · `Some(None)` = detach from node · `Some(Some(id))` = associate
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_double_opt"
    )]
    #[garde(skip)]
    pub node_id: Option<Option<NodeId>>,
    /// The sender's local date, sent with a completion (#281, 1.9). The
    /// server only knows the UTC date, and the next occurrence of a
    /// recurring task with no focus date is counted from "today": in a US
    /// evening UTC is already tomorrow, so a daily task skipped a day.
    /// Read through [`local_today`], which ignores an impossible value.
    /// Never stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub today: Option<NaiveDate>,
}

/// The date a request's sender calls today: their `hint` when it is a date
/// some time zone could be on right now (within a day of the UTC date),
/// otherwise the UTC date. A wrong or hostile hint can only move the
/// sender's own next occurrence by a day.
#[must_use]
pub fn local_today(hint: Option<NaiveDate>, utc_today: NaiveDate) -> NaiveDate {
    hint.filter(|d| (*d - utc_today).num_days().abs() <= 1)
        .unwrap_or(utc_today)
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
    if next == TaskStatus::Done {
        // The server counts a recurring task's next occurrence from the
        // viewer's day, not from UTC's (#281, 1.9).
        request.today = Some(today);
        if let Some(earlier) = focus_date.filter(|d| *d < today) {
            request.focus_date = Some(Some(today));
            revert.focus_date = Some(Some(earlier));
        }
    }
    DoneToggle {
        next,
        request,
        revert,
    }
}

/// Readable echo for a due-date input: the date spelled out plus where it
/// sits relative to today, so a mistyped segment (Oct → Jan) is obvious
/// before Save. Native `<input type="date">` segments can't be annotated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueHint {
    /// e.g. `"Fri, Oct 30 · in 7 days"`; the year appears when it isn't
    /// `today`'s year.
    pub label: String,
    /// `due` is before `today`.
    pub past: bool,
}

/// Build the [`DueHint`] for `due`, relative to the viewer's local `today`.
pub fn due_hint(due: NaiveDate, today: NaiveDate) -> DueHint {
    use chrono::Datelike;

    let date = if due.year() == today.year() {
        due.format("%a, %b %-d").to_string()
    } else {
        due.format("%a, %b %-d, %Y").to_string()
    };
    let days = (due - today).num_days();
    let n = days.unsigned_abs();
    let span = |count: u64, unit: &str| {
        let plural = if count == 1 { "" } else { "s" };
        if days > 0 {
            format!("in {count} {unit}{plural}")
        } else {
            format!("{count} {unit}{plural} ago")
        }
    };
    let relative = match days {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        _ if n <= 13 => span(n, "day"),
        _ if n < 56 => span(n / 7, "week"),
        // Rounded so 56 days reads "2 months", not "1 month".
        _ if n < 365 => span((n + 15) / 30, "month"),
        _ => span((n + 182) / 365, "year"),
    };
    DueHint {
        label: format!("{date} · {relative}"),
        past: days < 0,
    }
}

/// Should a due-date field warn "this date is in the past"? Only when the
/// date was changed (or newly set) to a day before `today`: editing just the
/// title of an already-overdue task must not nag. A warning, never a block —
/// back-dating is legitimate.
pub fn warn_past_due(
    new: Option<NaiveDate>,
    original: Option<NaiveDate>,
    today: NaiveDate,
) -> bool {
    new.is_some_and(|d| d < today && Some(d) != original)
}

/// What an editor's state picker shows once the task's own state has moved
/// from `base` to `now` outside the editor (the checkbox, another tab, or
/// the editor's own save landing). `sent` is the state the editor's last
/// Save sent, if it sent one.
///
/// The pick stays only while it is a choice still waiting to be saved:
/// made in the editor, not yet sent, on a task that still has a picker.
/// Otherwise the picker follows the task:
/// - untouched (`picked == base`);
/// - already sent: the task has moved since that Save, whether because it
///   landed or for another reason, so the pick is history, not intent
///   (re-sending it un-completed a task ticked after the save);
/// - the task is now done: a done task has no picker, and a pick must not
///   outlive the control that shows it.
///
/// The state differs from an editor's other fields in this: it changes
/// outside the editor, so "what the editor last sent" says nothing about
/// what the task is now.
#[must_use]
pub fn rebased_state(picked: &str, base: &str, now: &str, sent: Option<&str>) -> String {
    let untouched = picked == base;
    let already_sent = sent == Some(picked);
    let no_picker = now == TaskStatus::Done.as_str();
    if untouched || already_sent || no_picker {
        now
    } else {
        picked
    }
    .to_string()
}

/// The state an editor's save sends: the one picked, and only when it
/// differs from the task's own (`base`). An unknown word sends nothing.
#[must_use]
pub fn changed_state(picked: &str, base: &str) -> Option<TaskStatus> {
    (picked != base)
        .then(|| TaskStatus::parse(picked))
        .flatten()
}

/// The PATCH for an inline task edit, from its form values (the strings the
/// edit form's inputs hold). `None` when the title is blank. An empty due date
/// or recurrence clears it; an unknown priority is Medium. `status` is the
/// state picked in the editor, and only when it was changed there: an
/// edit of the title must not re-send the state the task happens to have.
/// Shared by every task list's inline editor (`task_actions::TaskEditDraft`).
pub fn task_edit_request(
    title: &str,
    priority: &str,
    due: &str,
    recurrence: &str,
    status: Option<TaskStatus>,
) -> Option<UpdateTaskRequest> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    Some(UpdateTaskRequest {
        title: Some(title.to_string()),
        status,
        priority: Some(TaskPriority::parse(priority).unwrap_or(TaskPriority::Medium)),
        due_date: Some(due.trim().parse::<NaiveDate>().ok()),
        recurrence: Some(RecurrenceRule::parse(recurrence)),
        ..Default::default()
    })
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
mod overdue_tests {
    use super::{TaskStatus, is_overdue};
    use chrono::NaiveDate;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).expect("date")
    }

    #[test]
    fn overdue_is_unfinished_and_strictly_past_due() {
        let today = day(10);
        assert!(is_overdue(&TaskStatus::Open, Some(day(9)), today));
        // Waiting is still to do: its due date is a fact either way.
        assert!(is_overdue(&TaskStatus::Waiting, Some(day(9)), today));
        // Due today is not overdue yet; no due date never is.
        assert!(!is_overdue(&TaskStatus::Open, Some(today), today));
        assert!(!is_overdue(&TaskStatus::Open, None, today));
        // Finished one way or the other: never overdue.
        assert!(!is_overdue(&TaskStatus::Done, Some(day(1)), today));
        assert!(!is_overdue(&TaskStatus::Cancelled, Some(day(1)), today));
    }
}

#[cfg(test)]
mod local_today_tests {
    use super::{TaskStatus, done_toggle, local_today};
    use chrono::NaiveDate;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).expect("date")
    }

    #[test]
    fn a_plausible_local_date_wins_over_utc() {
        // 9 pm in New York on the 9th is already the 10th in UTC.
        assert_eq!(local_today(Some(day(9)), day(10)), day(9));
        // Ahead of UTC, and the same day.
        assert_eq!(local_today(Some(day(11)), day(10)), day(11));
        assert_eq!(local_today(Some(day(10)), day(10)), day(10));
    }

    #[test]
    fn an_impossible_or_missing_date_falls_back_to_utc() {
        assert_eq!(local_today(None, day(10)), day(10));
        assert_eq!(local_today(Some(day(8)), day(10)), day(10));
        assert_eq!(local_today(Some(day(12)), day(10)), day(10));
    }

    #[test]
    fn completing_sends_the_viewers_day_and_reopening_does_not() {
        let done = done_toggle(&TaskStatus::Open, None, day(9));
        assert_eq!(done.request.today, Some(day(9)));
        let reopen = done_toggle(&TaskStatus::Done, None, day(9));
        assert_eq!(reopen.request.today, None);
        assert_eq!(done.revert.today, None);
    }
}

#[cfg(test)]
mod update_validation_tests {
    use super::UpdateTaskRequest;
    use garde::Validate;

    #[test]
    fn a_task_update_cannot_blank_or_overfill_the_title() {
        let with = |title: &str| UpdateTaskRequest {
            title: Some(title.to_string()),
            ..Default::default()
        };
        // #281, 1.5: these reached the database's CHECK and answered 500.
        assert!(with("").validate().is_err());
        assert!(with("  ").validate().is_err());
        assert!(with(&"a".repeat(501)).validate().is_err());
        assert!(with(&"a".repeat(500)).validate().is_ok());
        // The no-op PATCH, and one that doesn't touch the title.
        assert!(UpdateTaskRequest::default().validate().is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_parent_tells_no_project_from_an_unavailable_node() {
        let id = Some(NodeId(uuid::Uuid::nil()));
        assert_eq!(TaskParent::of(None, None), TaskParent::NoProject);
        let shown = TaskParent::of(id, Some("Plan".into()));
        assert_eq!(shown, TaskParent::Node("Plan".into()));
        assert_eq!((shown.label(), shown.icon()), ("Plan", "rocket_launch"));
        assert!(shown.is_reachable());
        // #230: a node the caller lost access to has no title, but the task
        // has a Project: it isn't "No Project".
        let gone = TaskParent::of(id, None);
        assert_eq!(gone, TaskParent::Unavailable);
        assert_eq!((gone.label(), gone.icon()), ("Node unavailable", "lock"));
        assert!(!gone.is_reachable());
        // No Project opens Data health, which can fix it.
        assert!(TaskParent::NoProject.is_reachable());
        assert_eq!(TaskParent::NoProject.label(), "No Project");
    }

    #[test]
    fn priority_labels_cover_every_variant_once_in_ascending_order() {
        // `ALL` is the one option list every priority control renders (#132).
        let labels: Vec<_> = TaskPriority::ALL.iter().map(TaskPriority::label).collect();
        assert_eq!(labels, ["Low", "Medium", "High"]);
        for p in TaskPriority::ALL {
            assert_eq!(TaskPriority::parse(p.as_str()).as_ref(), Some(p));
        }
    }

    #[test]
    fn terminal_statuses_are_done_and_cancelled() {
        assert!(TaskStatus::Done.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Open.is_terminal());
        assert!(!TaskStatus::Waiting.is_terminal());
    }

    #[test]
    fn toggle_completes_open_and_waiting() {
        assert_eq!(TaskStatus::Open.toggled(), TaskStatus::Done);
        assert_eq!(TaskStatus::Waiting.toggled(), TaskStatus::Done);
    }

    #[test]
    fn in_progress_is_read_as_open_and_never_written() {
        // S5a (#314): archives made before it, and old clients, still say
        // "in_progress".
        let read = |json: &str| serde_json::from_str::<TaskStatus>(json).expect(json);
        assert_eq!(read(r#""in_progress""#), TaskStatus::Open);
        assert_eq!(read(r#""open""#), TaskStatus::Open);
        assert_eq!(read(r#""waiting""#), TaskStatus::Waiting);
        assert_eq!(
            serde_json::to_string(&TaskStatus::Open).expect("json"),
            r#""open""#
        );
        // The database's words are strict: no row may hold it any more.
        assert_eq!(TaskStatus::parse("in_progress"), None);
        assert_eq!(TaskStatus::parse("waiting"), Some(TaskStatus::Waiting));
        assert!(serde_json::from_str::<TaskStatus>(r#""started""#).is_err());
    }

    fn task_in(status: TaskStatus, focus: Option<NaiveDate>, due: Option<NaiveDate>) -> Task {
        Task {
            id: TaskId(uuid::Uuid::from_u128(1)),
            node_id: None,
            owner_id: "alice".to_string(),
            title: "t".to_string(),
            status,
            priority: TaskPriority::Medium,
            focus_date: focus,
            due_date: due,
            recurrence: None,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        }
    }

    /// Every project a task can be on: none, and each declared state.
    fn projects() -> Vec<Option<ProjectState>> {
        std::iter::once(None)
            .chain(ProjectState::ALL.iter().copied().map(Some))
            .collect()
    }

    #[test]
    fn a_task_asks_for_attention_only_when_open_on_an_active_project_or_none() {
        for status in TaskStatus::ALL {
            for project in projects() {
                let expected = *status == TaskStatus::Open
                    && matches!(project, None | Some(ProjectState::Active));
                assert_eq!(
                    task_in(status.clone(), None, None).asks_for_attention(project),
                    expected,
                    "{status:?} on {project:?}"
                );
            }
        }
    }

    #[test]
    fn my_day_carries_over_only_what_asks_for_attention() {
        let today = day(12);
        for status in TaskStatus::ALL {
            for project in projects() {
                let what = format!("{status:?} on {project:?}");
                // Focused today: in My Day, whatever its state or its
                // project's. Its person put it there.
                assert!(
                    task_in(status.clone(), Some(today), None).in_my_day(today, project),
                    "{what} today"
                );
                // Focused on an earlier day: carried over only while it
                // still asks for attention.
                let carried = task_in(status.clone(), Some(day(9)), None);
                assert_eq!(
                    carried.in_my_day(today, project),
                    carried.asks_for_attention(project),
                    "{what} carried over"
                );
                // Never focused, or for a later day: not in today's.
                assert!(!task_in(status.clone(), None, None).in_my_day(today, project));
                assert!(!task_in(status.clone(), Some(day(20)), None).in_my_day(today, project));
            }
        }
        // Spelled out once: an open task on a parked project stays behind.
        let open = task_in(TaskStatus::Open, Some(day(9)), None);
        assert!(open.in_my_day(today, None));
        assert!(open.in_my_day(today, Some(ProjectState::Active)));
        assert!(!open.in_my_day(today, Some(ProjectState::Parked)));
        assert!(!open.in_my_day(today, Some(ProjectState::Complete)));
    }

    #[test]
    fn the_overdue_zone_takes_only_what_asks_for_attention() {
        let today = day(12);
        let late = Some(day(9));
        assert!(task_in(TaskStatus::Open, None, late).in_overdue_zone(today, None));
        // A waiting task past its date: the date is a fact, but it isn't
        // put in front of its person as something to do now.
        let waiting = task_in(TaskStatus::Waiting, None, late);
        assert!(waiting.is_overdue(today));
        assert!(!waiting.in_overdue_zone(today, None));
        for closed in [TaskStatus::Done, TaskStatus::Cancelled] {
            assert!(!task_in(closed, None, late).in_overdue_zone(today, None));
        }
        assert!(!task_in(TaskStatus::Open, None, Some(today)).in_overdue_zone(today, None));
        assert!(!task_in(TaskStatus::Open, None, None).in_overdue_zone(today, None));
        // Nor is an open, late task of a project that is parked or closed
        // (S6a): its date stays red on its row, and it stays in the Backlog.
        let open_late = task_in(TaskStatus::Open, None, late);
        for project in projects() {
            assert_eq!(
                open_late.in_overdue_zone(today, project),
                matches!(project, None | Some(ProjectState::Active)),
                "{project:?}"
            );
            assert!(open_late.is_overdue(today));
        }
    }

    #[test]
    fn a_feed_task_carries_its_projects_state_into_the_rules() {
        let today = day(12);
        let on = |project: Option<ProjectState>| MyDayTask {
            task: task_in(TaskStatus::Open, Some(day(9)), Some(day(9))),
            node_title: project.map(|_| "p".to_string()),
            project_state: project,
        };
        for project in projects() {
            let t = on(project);
            assert_eq!(t.in_my_day(today), t.task.in_my_day(today, project));
            assert_eq!(
                t.in_overdue_zone(today),
                t.task.in_overdue_zone(today, project)
            );
            assert_eq!(
                t.quiet_project(),
                project.filter(|s| *s != ProjectState::Active),
                "{project:?}"
            );
        }
        // A feed from before S6a has no such field: nothing is held back.
        let old: MyDayTask = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::from_u128(1), "node_id": null, "owner_id": "alice",
            "title": "t", "status": "open", "priority": "medium",
            "focus_date": null, "due_date": null, "recurrence": null, "sort_order": 0,
            "created_at": Utc::now(), "updated_at": Utc::now(), "node_title": null,
        }))
        .expect("an older feed row");
        assert_eq!(old.project_state, None);
    }

    #[test]
    fn only_an_open_task_is_actionable_and_done_is_not_picked_by_hand() {
        for status in TaskStatus::ALL {
            assert_eq!(
                status.is_actionable(),
                *status == TaskStatus::Open,
                "{status:?}"
            );
        }
        assert_eq!(
            TaskStatus::PICKABLE,
            [TaskStatus::Open, TaskStatus::Waiting, TaskStatus::Cancelled]
        );
    }

    #[test]
    fn every_status_has_its_own_label() {
        let labels: Vec<&str> = TaskStatus::ALL.iter().map(TaskStatus::label).collect();
        assert_eq!(labels, ["Open", "Waiting", "Done", "Cancelled"]);
    }

    #[test]
    fn a_task_from_before_s5a_has_no_completion_date() {
        // An archive of format 8 or older has no `completed_at`, and may
        // say `in_progress`.
        let task: Task = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::from_u128(1),
            "node_id": null,
            "owner_id": "alice",
            "title": "old",
            "status": "in_progress",
            "priority": "medium",
            "focus_date": null,
            "due_date": null,
            "recurrence": null,
            "sort_order": 0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
        }))
        .expect("an old task");
        assert_eq!(task.status, TaskStatus::Open);
        assert_eq!(task.completed_at, None);

        // A restore keeps a completion date on a done task only.
        let at = Utc::now();
        for (status, kept) in [
            (TaskStatus::Done, Some(at)),
            (TaskStatus::Open, None),
            (TaskStatus::Waiting, None),
            (TaskStatus::Cancelled, None),
        ] {
            let t = Task {
                status,
                completed_at: Some(at),
                ..task.clone()
            };
            assert_eq!(t.restorable_completed_at(), kept, "{:?}", t.status);
        }
        assert_eq!(
            Task {
                status: TaskStatus::Done,
                ..task
            }
            .restorable_completed_at(),
            None,
            "a done task from before S5a keeps its empty date"
        );
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
            let t = done_toggle(&TaskStatus::Waiting, focus, day(12));
            assert!(t.completes());
            assert_eq!(t.request.focus_date, None, "focus {focus:?}");
            assert_eq!(t.revert.focus_date, None);
            // Undo returns to Waiting, not a generic Open.
            assert_eq!(t.revert.status, Some(TaskStatus::Waiting));
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

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn due_hint_relative_bands() {
        let today = date(2026, 10, 23); // a Friday
        let cases = [
            (date(2026, 10, 23), "Fri, Oct 23 · today"),
            (date(2026, 10, 24), "Sat, Oct 24 · tomorrow"),
            (date(2026, 10, 22), "Thu, Oct 22 · yesterday"),
            (date(2026, 10, 30), "Fri, Oct 30 · in 7 days"),
            (date(2026, 11, 5), "Thu, Nov 5 · in 13 days"),
            (date(2026, 11, 6), "Fri, Nov 6 · in 2 weeks"),
            (date(2026, 10, 9), "Fri, Oct 9 · 2 weeks ago"),
            (date(2026, 12, 18), "Fri, Dec 18 · in 2 months"),
            (date(2027, 10, 23), "Sat, Oct 23, 2027 · in 1 year"),
        ];
        for (due, want) in cases {
            assert_eq!(due_hint(due, today).label, want, "{due}");
        }
    }

    #[test]
    fn due_hint_makes_the_month_slip_obvious() {
        // The reported accident: a stray "1" in the Month segment turned
        // Oct 30 into Jan 30 — the echo must read as clearly wrong.
        let hint = due_hint(date(2026, 1, 30), date(2026, 10, 23));
        assert_eq!(hint.label, "Fri, Jan 30 · 9 months ago");
        assert!(hint.past);
        assert!(!due_hint(date(2026, 10, 23), date(2026, 10, 23)).past);
    }

    #[test]
    fn due_hint_shows_year_outside_current_year() {
        let hint = due_hint(date(2025, 12, 31), date(2026, 1, 2));
        assert_eq!(hint.label, "Wed, Dec 31, 2025 · 2 days ago");
    }

    #[test]
    fn past_due_warning_only_for_changed_past_dates() {
        let today = date(2026, 10, 23);
        let past = Some(date(2026, 10, 1));
        // Changed to the past → warn (new task or edited date).
        assert!(warn_past_due(past, None, today));
        assert!(warn_past_due(past, Some(date(2026, 11, 1)), today));
        // An already-overdue task whose date was left alone → no nagging.
        assert!(!warn_past_due(past, past, today));
        // Today, future, or cleared → fine.
        assert!(!warn_past_due(Some(today), None, today));
        assert!(!warn_past_due(Some(date(2026, 12, 1)), past, today));
        assert!(!warn_past_due(None, past, today));
    }

    #[test]
    fn recurrence_advance_clamps_to_month_end_without_drift() {
        let d = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        assert_eq!(
            RecurrenceRule::Monthly.advance(d(2026, 1, 31)),
            d(2026, 2, 28)
        );
        assert_eq!(
            RecurrenceRule::Monthly.advance(d(2028, 1, 31)),
            d(2028, 2, 29)
        );
        assert_eq!(
            RecurrenceRule::Monthly.advance(d(2026, 3, 31)),
            d(2026, 4, 30)
        );
        assert_eq!(
            RecurrenceRule::Monthly.advance(d(2026, 12, 15)),
            d(2027, 1, 15)
        );
        assert_eq!(
            RecurrenceRule::Yearly.advance(d(2028, 2, 29)),
            d(2029, 2, 28)
        );
        assert_eq!(
            RecurrenceRule::Weekly.advance(d(2026, 10, 30)),
            d(2026, 11, 6)
        );
        assert_eq!(
            RecurrenceRule::Biweekly.advance(d(2026, 12, 25)),
            d(2027, 1, 8)
        );
    }

    #[test]
    fn task_edit_request_maps_form_values() {
        let r = task_edit_request("  Ship it  ", "high", "2026-11-03", "weekly", None).unwrap();
        assert_eq!(r.status, None, "an unchanged state is not sent");
        assert_eq!(r.title.as_deref(), Some("Ship it"));
        assert_eq!(r.priority, Some(TaskPriority::High));
        assert_eq!(r.due_date, Some(NaiveDate::from_ymd_opt(2026, 11, 3)));
        assert_eq!(r.recurrence, Some(Some(RecurrenceRule::Weekly)));
        // Empty due / recurrence clear (Some(None)); unknown priority = Medium.
        let r = task_edit_request("x", "bogus", "", "", Some(TaskStatus::Waiting)).unwrap();
        assert_eq!(r.status, Some(TaskStatus::Waiting));
        assert_eq!(r.priority, Some(TaskPriority::Medium));
        assert_eq!(r.due_date, Some(None));
        assert_eq!(r.recurrence, Some(None));
        // Blank title: no request at all.
        assert!(task_edit_request("   ", "high", "", "", None).is_none());
    }

    #[test]
    fn an_untouched_picker_follows_the_task_and_a_choice_stays() {
        // Untouched: the task was set to waiting elsewhere; the editor follows.
        assert_eq!(rebased_state("open", "open", "waiting", None), "waiting");
        // A choice made in the editor and not yet saved survives the task
        // moving under it.
        assert_eq!(
            rebased_state("cancelled", "open", "waiting", None),
            "cancelled"
        );
        // The editor's own save landing: the choice is now the task's.
        assert_eq!(
            rebased_state("waiting", "open", "waiting", Some("waiting")),
            "waiting"
        );
    }

    #[test]
    fn a_pick_already_sent_or_on_a_done_task_does_not_come_back() {
        // Saved as waiting, then ticked done with the checkbox: the editor
        // must not hold "waiting" and send it with the next title edit.
        assert_eq!(
            rebased_state("waiting", "open", "done", Some("waiting")),
            "done"
        );
        // Saved as waiting, then cancelled elsewhere: same.
        assert_eq!(
            rebased_state("waiting", "open", "cancelled", Some("waiting")),
            "cancelled"
        );
        // An unsaved pick, and the task is ticked done in another tab: a
        // done task has no picker, so the pick goes with it.
        assert_eq!(rebased_state("waiting", "open", "done", None), "done");
        // A different pick from the one sent is still a choice.
        assert_eq!(
            rebased_state("cancelled", "open", "waiting", Some("waiting")),
            "cancelled"
        );
    }

    #[test]
    fn a_state_is_sent_only_when_it_differs_from_the_tasks_own() {
        assert_eq!(changed_state("open", "open"), None);
        assert_eq!(changed_state("waiting", "open"), Some(TaskStatus::Waiting));
        // A save that failed leaves the task as it was, so the retry
        // sends the state again: nothing "already sent" is remembered.
        assert_eq!(changed_state("waiting", "open"), Some(TaskStatus::Waiting));
        // Once the task has it, there is nothing to send.
        assert_eq!(changed_state("waiting", "waiting"), None);
        assert_eq!(changed_state("bogus", "open"), None);
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
