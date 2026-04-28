use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post, put},
};
use chrono::{Datelike, NaiveDate};
use common::{
    auth::AuthClaims,
    id::{NodeId, TaskId},
    markdown::extract_section,
    node::{NodeListParams, NodeType},
    task::{
        CreateTaskRequest, MyDayTask, ProjectDashboardEntry, RecurrenceRule, ReorderTasksRequest,
        Task, TaskCounts, TaskStatus, UpdateTaskRequest,
    },
};
use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor, require_viewer},
    error::ApiError,
    state::AppState,
};

/// Mounts under `/nodes/:node_id/tasks` and `/tasks`.
pub fn node_task_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
}

pub fn task_router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_standalone_task))
        .route("/inbox", get(list_inbox))
        .route("/all", get(list_all_open))
        .route("/{id}", patch(update_task).delete(delete_task))
        .route("/reorder", put(reorder_tasks))
}

#[derive(Deserialize)]
struct MyDayQuery {
    date: Option<NaiveDate>,
}

pub fn my_day_router() -> Router<AppState> {
    Router::new().route("/", get(my_day))
}

pub fn calendar_router() -> Router<AppState> {
    Router::new().route("/", get(calendar_handler))
}

pub fn dashboard_router() -> Router<AppState> {
    Router::new().route("/", get(project_dashboard))
}

// ── Recurrence helpers ────────────────────────────────────────────────────────

/// Advance `d` by one recurrence interval. Falls back to `d` on edge cases
/// (e.g. Feb 29 in non-leap year advances to Feb 28).
fn advance_date(d: NaiveDate, rule: &RecurrenceRule) -> NaiveDate {
    match rule {
        RecurrenceRule::Daily    => d + chrono::Duration::days(1),
        RecurrenceRule::Weekly   => d + chrono::Duration::weeks(1),
        RecurrenceRule::Biweekly => d + chrono::Duration::weeks(2),
        RecurrenceRule::Monthly  => {
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1u32)
            } else {
                (d.year(), d.month() + 1)
            };
            NaiveDate::from_ymd_opt(y, m, d.day())
                .or_else(|| NaiveDate::from_ymd_opt(y, m, 28))
                .unwrap_or(d)
        }
        RecurrenceRule::Yearly => {
            NaiveDate::from_ymd_opt(d.year() + 1, d.month(), d.day())
                .or_else(|| NaiveDate::from_ymd_opt(d.year() + 1, d.month(), 28))
                .unwrap_or(d)
        }
    }
}

fn status_is_done(s: &TaskStatus) -> bool {
    matches!(s, TaskStatus::Done | TaskStatus::Cancelled)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn list_tasks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<Task>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let tasks = state
        .tasks
        .list_for_node(NodeId(node_id), &claims.sub)
        .await?;
    Ok(Json(tasks))
}

async fn create_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let task = state
        .tasks
        .create(Some(NodeId(node_id)), &claims.sub, req)
        .await?;
    Ok((StatusCode::CREATED, Json(task)))
}

/// POST /tasks — create a standalone (inbox) task with no parent node.
async fn create_standalone_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    // If a parent node is specified, verify the caller has editor access.
    if let Some(nid) = req.node_id {
        require_editor(state.permissions.as_ref(), &claims, nid).await?;
    }
    let node_id = req.node_id;
    let task = state.tasks.create(node_id, &claims.sub, req).await?;
    Ok((StatusCode::CREATED, Json(task)))
}

/// GET /tasks/inbox — standalone tasks (node_id IS NULL) for the authenticated user.
async fn list_inbox(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<Task>>, ApiError> {
    let tasks = state.tasks.list_inbox(&claims.sub).await?;
    Ok(Json(tasks))
}

/// Backlog feed for the My Day Kanban — every open task across every node,
/// joined with parent node title.  See `TaskRepo::list_open_for_owner` for
/// the sort contract.
async fn list_all_open(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<MyDayTask>>, ApiError> {
    let tasks = state.tasks.list_open_for_owner(&claims.sub).await?;
    Ok(Json(tasks))
}

async fn update_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<Task>, ApiError> {
    // Fetch the task to verify ownership / node permission.
    let existing = state.tasks.get(TaskId(id)).await?;
    if existing.owner_id != claims.sub && !is_admin(&claims) {
        // Not the owner — check node-level permission if attached to a node.
        if let Some(nid) = existing.node_id {
            require_editor(state.permissions.as_ref(), &claims, nid).await?;
        } else {
            return Err(ApiError::Forbidden("access denied".to_string()));
        }
    }

    // Check if this update transitions to Done — needed for recurrence.
    let becoming_done = req
        .status
        .as_ref()
        .is_some_and(|s| matches!(s, TaskStatus::Done));

    // Use the already-fetched task for recurrence check.
    let pre = if becoming_done && !status_is_done(&existing.status) {
        Some(existing)
    } else {
        None
    };

    let updated = state.tasks.update(TaskId(id), req).await?;

    // If the task had recurrence and was not already done, schedule next occurrence.
    if let Some(pre_task) = pre
        && let Some(rule) = &pre_task.recurrence
        && !status_is_done(&pre_task.status)
    {
        let today = chrono::Utc::now().date_naive();
        let base_focus = pre_task.focus_date.unwrap_or(today);
        let next_focus = advance_date(base_focus, rule);
        let next_due = pre_task.due_date.map(|d| advance_date(d, rule));
        let next_req = CreateTaskRequest {
            title: pre_task.title.clone(),
            node_id: pre_task.node_id,
            status: Some(TaskStatus::Open),
            priority: Some(pre_task.priority.clone()),
            focus_date: Some(next_focus),
            due_date: next_due,
            recurrence: Some(rule.clone()),
        };
        // Best-effort — ignore errors so the Done update still succeeds.
        let next_node_id = next_req.node_id;
        let _ = state
            .tasks
            .create(next_node_id, &claims.sub, next_req)
            .await;
    }

    Ok(Json(updated))
}

async fn delete_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let existing = state.tasks.get(TaskId(id)).await?;
    if existing.owner_id != claims.sub && !is_admin(&claims) {
        if let Some(nid) = existing.node_id {
            require_editor(state.permissions.as_ref(), &claims, nid).await?;
        } else {
            return Err(ApiError::Forbidden("access denied".to_string()));
        }
    }
    state.tasks.delete(TaskId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /tasks/reorder — bulk update sort_order for drag-to-reorder in My Day.
async fn reorder_tasks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<ReorderTasksRequest>,
) -> Result<StatusCode, ApiError> {
    let updates: Vec<(TaskId, i32)> = req
        .tasks
        .iter()
        .map(|e| (e.id, e.sort_order))
        .collect();
    state.tasks.reorder(&updates, &claims.sub).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /my-day?date=YYYY-MM-DD  (defaults to today UTC if omitted)
async fn my_day(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(q): Query<MyDayQuery>,
) -> Result<Json<Vec<MyDayTask>>, ApiError> {
    let date = q.date.unwrap_or_else(|| chrono::Utc::now().date_naive());
    let tasks = state.tasks.list_my_day(&claims.sub, date).await?;
    Ok(Json(tasks))
}

#[derive(Deserialize)]
struct CalendarParams {
    year: i32,
    month: u32,
}

/// GET /calendar?year=YYYY&month=M — tasks with due_date in the given month.
async fn calendar_handler(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<CalendarParams>,
) -> Result<Json<Vec<MyDayTask>>, ApiError> {
    use chrono::NaiveDate;
    let from = NaiveDate::from_ymd_opt(params.year, params.month, 1)
        .ok_or_else(|| ApiError::Validation("invalid year/month".into()))?;
    let to = if params.month == 12 {
        NaiveDate::from_ymd_opt(params.year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(params.year, params.month + 1, 1)
    }
    .ok_or_else(|| ApiError::Validation("invalid year/month".into()))?
    .pred_opt()
    .ok_or_else(|| ApiError::Validation("date underflow".into()))?;

    let tasks = state.tasks.list_by_due_range(&claims.sub, from, to).await?;
    Ok(Json(tasks))
}

/// Maximum open tasks to include per project card.
const DASHBOARD_TASK_LIMIT: i64 = 10;

/// GET /dashboard/projects — all project nodes with aggregated task counts,
/// a rendered status section from the node body, and top open tasks.
async fn project_dashboard(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<ProjectDashboardEntry>>, ApiError> {
    // Fetch all project nodes for this owner.
    let params = NodeListParams {
        node_type: Some(NodeType::Project),
        status: None,
        tag_id: None,
        owner_id: Some(claims.sub.clone()),
        subject_id: None,
        page: Some(1),
        per_page: Some(500),
        include_archived: false,
    };
    let (projects, _) = state.nodes.list(params).await?;

    if projects.is_empty() {
        return Ok(Json(vec![]));
    }

    let node_ids: Vec<NodeId> = projects.iter().map(|n| n.id).collect();

    // Fetch counts, open tasks, and per-node task activity in parallel.
    let (counts_result, open_result, activity_result) = tokio::join!(
        state.tasks.counts_for_nodes(&node_ids),
        state.tasks.list_open_for_nodes(&node_ids, DASHBOARD_TASK_LIMIT),
        state.tasks.max_task_updated_for_nodes(&node_ids),
    );

    let counts_map: std::collections::HashMap<NodeId, TaskCounts> =
        counts_result?.into_iter().collect();
    let open_map: std::collections::HashMap<NodeId, (Vec<_>, bool)> = open_result?
        .into_iter()
        .map(|(nid, tasks, more)| (nid, (tasks, more)))
        .collect();
    let activity_map: std::collections::HashMap<NodeId, chrono::DateTime<chrono::Utc>> =
        activity_result?.into_iter().collect();

    let empty = TaskCounts { open: 0, in_progress: 0, done: 0, cancelled: 0 };
    let mut entries: Vec<ProjectDashboardEntry> = projects
        .into_iter()
        .map(|n| {
            let node_status = format!("{:?}", n.status).to_lowercase();
            let task_counts = counts_map.get(&n.id).cloned().unwrap_or_else(|| empty.clone());
            let status_section = n.body.as_deref().and_then(|b| extract_section(b, "status"));
            let (open_tasks, has_more_tasks) = open_map
                .get(&n.id)
                .cloned()
                .unwrap_or_default();
            let last_activity_at = activity_map
                .get(&n.id)
                .copied()
                .map_or(n.updated_at, |t| t.max(n.updated_at));
            ProjectDashboardEntry {
                node_id: n.id,
                title: n.title,
                node_status,
                task_counts,
                status_section,
                open_tasks,
                has_more_tasks,
                last_activity_at,
            }
        })
        .collect();

    // Most recently active projects first — keeps the 2–3 projects the
    // user is currently working on at the top of the dashboard.
    entries.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));

    Ok(Json(entries))
}
