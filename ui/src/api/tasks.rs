use common::{
    id::{NodeId, TaskId},
    task::{
        CreateTaskRequest, MyDayTask, ProjectDashboardEntry, ReorderTaskEntry, ReorderTasksRequest,
        Task, UpdateTaskRequest,
    },
};

use super::{delete_empty, get_json, patch_json, post_json, put_empty};
use crate::error::UiError;

pub async fn fetch_tasks(node_id: NodeId) -> Result<Vec<Task>, UiError> {
    get_json(&format!("/nodes/{node_id}/tasks")).await
}

pub async fn list_inbox() -> Result<Vec<Task>, UiError> {
    get_json("/tasks/inbox").await
}

/// Backlog feed for the My Day Kanban — every open task across every node,
/// joined with parent node title, sorted due_date NULLS LAST then priority
/// then created_at.  See `TaskRepo::list_open_for_owner`.
pub async fn list_open_tasks() -> Result<Vec<MyDayTask>, UiError> {
    get_json("/tasks/all").await
}

pub async fn create_standalone_task(req: &CreateTaskRequest) -> Result<Task, UiError> {
    post_json("/tasks", req).await
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
