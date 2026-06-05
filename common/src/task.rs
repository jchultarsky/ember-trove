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
fn deser_double_opt<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
