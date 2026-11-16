use common::{
    id::{NodeId, TaskId},
    task::{
        CreateTaskRequest, ListedTask, MyDayTask, ProjectDashboardEntry, ReorderTaskEntry,
        ReorderTasksRequest, Task, UpdateTaskRequest,
    },
    task_dependency::{AddDependencyRequest, TaskDependencies, TaskLink},
};

use super::{delete_empty, delete_json, get_json, patch_json, post_json, put_empty};
use crate::error::UiError;

/// A node's tasks, each with how many tasks it still waits for (S5d).
pub async fn fetch_tasks(node_id: NodeId) -> Result<Vec<ListedTask>, UiError> {
    get_json(&format!("/nodes/{node_id}/tasks")).await
}

// ── Dependencies (S5d) ──────────────────────────────────────────────────

/// The tasks `task_id` waits for, the tasks that wait for it, and whether
/// the reader may change them.
pub async fn fetch_task_dependencies(task_id: TaskId) -> Result<TaskDependencies, UiError> {
    get_json(&format!("/tasks/{task_id}/dependencies")).await
}

/// Make `task_id` wait for `depends_on`. Answers the fresh lists.
pub async fn add_task_dependency(
    task_id: TaskId,
    depends_on: TaskId,
) -> Result<TaskDependencies, UiError> {
    post_json(
        &format!("/tasks/{task_id}/dependencies"),
        &AddDependencyRequest { depends_on },
    )
    .await
}

/// Stop `task_id` waiting for `other`. Answers the fresh lists.
pub async fn remove_task_dependency(
    task_id: TaskId,
    other: TaskId,
) -> Result<TaskDependencies, UiError> {
    delete_json(&format!("/tasks/{task_id}/dependencies/{other}")).await
}

/// Remove every prerequisite of `task_id` that the reader isn't shown (one
/// in the trash, or on a node they can't view). Answers the fresh lists.
pub async fn remove_task_dependencies_not_shown(
    task_id: TaskId,
) -> Result<TaskDependencies, UiError> {
    delete_json(&format!("/tasks/{task_id}/dependencies/not-shown")).await
}

/// What the dependency picker offers for `q` (nothing typed: the nearest
/// tasks, the same Project's first).
pub async fn dependency_candidates(task_id: TaskId, q: &str) -> Result<Vec<TaskLink>, UiError> {
    let encoded: String = js_sys::encode_uri_component(q.trim()).into();
    get_json(&format!(
        "/tasks/{task_id}/dependencies/candidates?q={encoded}"
    ))
    .await
}

/// Backlog feed for the My Day Kanban — every open task across every node,
/// joined with parent node title, sorted due_date NULLS LAST then priority
/// then created_at.  See `TaskRepo::list_open_for_owner`.
pub async fn list_open_tasks() -> Result<Vec<MyDayTask>, UiError> {
    get_json("/tasks/all").await
}

pub async fn create_task(node_id: NodeId, req: &CreateTaskRequest) -> Result<Task, UiError> {
    post_json(&format!("/nodes/{node_id}/tasks"), req).await
}

pub async fn update_task(task_id: TaskId, req: &UpdateTaskRequest) -> Result<Task, UiError> {
    patch_json(&format!("/tasks/{task_id}"), req).await
}

pub async fn delete_task(task_id: TaskId) -> Result<(), UiError> {
    delete_empty(&format!("/tasks/{task_id}")).await
}

/// `POST /api/tasks/:id/restore` — un-delete a soft-deleted task (undo toast).
pub async fn restore_task(task_id: TaskId) -> Result<Task, UiError> {
    post_json(&format!("/tasks/{task_id}/restore"), &serde_json::json!({})).await
}

pub async fn reorder_tasks(entries: &[(TaskId, i32)]) -> Result<(), UiError> {
    let req = ReorderTasksRequest {
        tasks: entries
            .iter()
            .map(|(id, order)| ReorderTaskEntry {
                id: *id,
                sort_order: *order,
            })
            .collect(),
    };
    put_empty("/tasks/reorder", &req).await
}

pub async fn fetch_project_dashboard() -> Result<Vec<ProjectDashboardEntry>, UiError> {
    get_json("/dashboard/projects").await
}

pub async fn fetch_my_day(date: chrono::NaiveDate) -> Result<Vec<MyDayTask>, UiError> {
    get_json(&format!("/my-day?date={date}")).await
}

pub async fn fetch_calendar_tasks(year: i32, month: u32) -> Result<Vec<MyDayTask>, UiError> {
    get_json(&format!("/calendar?year={year}&month={month}")).await
}
