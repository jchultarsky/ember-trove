use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post, put},
};
use chrono::NaiveDate;
use common::{
    auth::AuthClaims,
    id::{NodeId, TaskId},
    markdown::extract_section,
    node::{NodeListParams, NodeType},
    ontology::Capability,
    task::{
        CreateTaskRequest, MyDayTask, ProjectDashboardEntry, ReorderTasksRequest, Task, TaskCounts,
        TaskStatus, UpdateTaskRequest,
    },
    webhook::{EVENT_TASK_CREATED, EVENT_TASK_DELETED, EVENT_TASK_UPDATED},
};
use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor, require_viewer},
    error::ApiError,
    routes::capability::require_capability,
    state::AppState,
    webhook_dispatch::dispatch,
};

/// Mounts under `/nodes/:node_id/tasks` and `/tasks`.
pub fn node_task_router() -> Router<AppState> {
    Router::new().route("/", get(list_tasks).post(create_task))
}

pub fn task_router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_standalone_task))
        .route("/inbox", get(list_inbox))
        .route("/all", get(list_all_open))
        .route("/{id}", patch(update_task).delete(delete_task))
        .route("/{id}/restore", post(restore_task))
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

/// Mounted at `/dashboard/activity` — recent activity recap for the
/// home Dashboard's "What changed today / yesterday" section (v2.9.0).
pub fn dashboard_activity_router() -> Router<AppState> {
    Router::new().route("/", get(dashboard_activity))
}

// ── Recurrence helpers ────────────────────────────────────────────────────────

/// What a status change does to a recurring task's series.
#[derive(Debug)]
enum SeriesEffect {
    /// Nothing to schedule or retract.
    Unchanged,
    /// Completed: schedule the next occurrence. `owner_id` is the series
    /// owner — never the caller, who may be a node editor completing someone
    /// else's task.
    ScheduleNext {
        owner_id: String,
        req: CreateTaskRequest,
    },
    /// Reopened a completed instance: the occurrence its completion scheduled
    /// is now a duplicate-in-waiting, so retract it.
    RetractNext,
}

/// Decide the series side effect of moving `pre` to `new_status`. Pure, so
/// the transition table is unit-tested without a repo.
fn series_effect(pre: &Task, new_status: Option<&TaskStatus>, today: NaiveDate) -> SeriesEffect {
    let Some(new_status) = new_status else {
        return SeriesEffect::Unchanged;
    };
    match (&pre.status, new_status) {
        (from, TaskStatus::Done) if !from.is_terminal() => {
            let Some(rule) = &pre.recurrence else {
                return SeriesEffect::Unchanged;
            };
            let base_focus = pre.focus_date.unwrap_or(today);
            SeriesEffect::ScheduleNext {
                owner_id: pre.owner_id.clone(),
                req: CreateTaskRequest {
                    title: pre.title.clone(),
                    node_id: pre.node_id,
                    status: Some(TaskStatus::Open),
                    priority: Some(pre.priority.clone()),
                    focus_date: Some(rule.advance(base_focus)),
                    due_date: pre.due_date.map(|d| rule.advance(d)),
                    recurrence: Some(rule.clone()),
                },
            }
        }
        // Keyed on the link, not on `pre.recurrence`: the rule may have been
        // cleared since the completion that scheduled the occurrence.
        (TaskStatus::Done, TaskStatus::Open | TaskStatus::InProgress) => SeriesEffect::RetractNext,
        _ => SeriesEffect::Unchanged,
    }
}

/// The next occurrence goes on the task's node as it is *after* the
/// update, so a task completed and moved in one request takes its series
/// with it (#216). Otherwise it would land on the node the task just left.
fn scheduled_on(
    mut req: CreateTaskRequest,
    node_after_update: Option<NodeId>,
) -> CreateTaskRequest {
    req.node_id = node_after_update;
    req
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
    require_capability(&state, NodeId(node_id), Capability::Tasks).await?;
    let task = state
        .tasks
        .create(Some(NodeId(node_id)), &claims.sub, req)
        .await?;
    dispatch(
        state.webhooks.clone(),
        EVENT_TASK_CREATED,
        task.node_id,
        &claims.sub,
        &task.owner_id,
    );
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
        require_capability(&state, nid, Capability::Tasks).await?;
    }
    let node_id = req.node_id;
    let task = state.tasks.create(node_id, &claims.sub, req).await?;
    dispatch(
        state.webhooks.clone(),
        EVENT_TASK_CREATED,
        task.node_id,
        &claims.sub,
        &task.owner_id,
    );
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

    // SECURITY: moving a task onto a node (node_id: Some(Some(_))) requires
    // editor access to the DESTINATION — the check above only covers the
    // task's current node. Mirrors create_standalone_task.
    if let Some(Some(dest)) = req.node_id {
        require_editor(state.permissions.as_ref(), &claims, dest).await?;
        // §3.2 (#216): only onto a Project. Staying on the current node is
        // not a move, so a task already on a non-Project stays editable.
        if existing.node_id != Some(dest) {
            require_capability(&state, dest, Capability::Tasks).await?;
        }
    }

    let effect = series_effect(
        &existing,
        req.status.as_ref(),
        chrono::Utc::now().date_naive(),
    );

    let updated = state.tasks.update(TaskId(id), req).await?;

    // Best-effort: the status update itself has succeeded either way.
    match effect {
        SeriesEffect::Unchanged => {}
        SeriesEffect::ScheduleNext { owner_id, req } => {
            let req = scheduled_on(req, updated.node_id);
            if let Err(e) = state
                .tasks
                .create_occurrence(TaskId(id), &owner_id, req)
                .await
            {
                tracing::warn!(task_id = %id, error = %e, "scheduling next occurrence failed");
            }
        }
        SeriesEffect::RetractNext => {
            if let Err(e) = state
                .tasks
                .retract_occurrence(TaskId(id), &existing.owner_id)
                .await
            {
                tracing::warn!(task_id = %id, error = %e, "retracting next occurrence failed");
            }
        }
    }

    dispatch(
        state.webhooks.clone(),
        EVENT_TASK_UPDATED,
        updated.node_id,
        &claims.sub,
        &updated.owner_id,
    );
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
    let (owner_id, node_id) = (existing.owner_id.clone(), existing.node_id);
    state.tasks.delete(TaskId(id)).await?;
    dispatch(
        state.webhooks.clone(),
        EVENT_TASK_DELETED,
        node_id,
        &claims.sub,
        &owner_id,
    );
    Ok(StatusCode::NO_CONTENT)
}

/// POST /tasks/:id/restore — un-delete a soft-deleted task. Backs the undo
/// toast; authorization mirrors delete (owner, admin, or editor of the parent
/// node).
async fn restore_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Task>, ApiError> {
    let existing = state.tasks.get_deleted(TaskId(id)).await?;
    if existing.owner_id != claims.sub && !is_admin(&claims) {
        if let Some(nid) = existing.node_id {
            require_editor(state.permissions.as_ref(), &claims, nid).await?;
        } else {
            return Err(ApiError::Forbidden("access denied".to_string()));
        }
    }
    let restored = state.tasks.restore(TaskId(id)).await?;
    dispatch(
        state.webhooks.clone(),
        EVENT_TASK_UPDATED,
        restored.node_id,
        &claims.sub,
        &restored.owner_id,
    );
    Ok(Json(restored))
}

/// PUT /tasks/reorder — bulk update sort_order for drag-to-reorder in My Day.
async fn reorder_tasks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<ReorderTasksRequest>,
) -> Result<StatusCode, ApiError> {
    let updates: Vec<(TaskId, i32)> = req.tasks.iter().map(|e| (e.id, e.sort_order)).collect();
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
/// its project status (#143) plus any status narrative left in the body,
/// and top open tasks.
async fn project_dashboard(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<ProjectDashboardEntry>>, ApiError> {
    // Fetch all project nodes for this owner.
    let params = NodeListParams {
        node_type: Some(NodeType::Project),
        status: None,
        project_status: None,
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

    // Fetch counts, open tasks, per-node task activity, and Area
    // parents (PARA grouping, v2.9.0) in parallel.
    let (counts_result, open_result, activity_result, area_result) = tokio::join!(
        state.tasks.counts_for_nodes(&node_ids),
        state
            .tasks
            .list_open_for_nodes(&node_ids, DASHBOARD_TASK_LIMIT),
        state.tasks.max_task_updated_for_nodes(&node_ids),
        state.nodes.area_for_nodes(&node_ids),
    );

    let counts_map: std::collections::HashMap<NodeId, TaskCounts> =
        counts_result?.into_iter().collect();
    let open_map: std::collections::HashMap<NodeId, (Vec<_>, bool)> = open_result?
        .into_iter()
        .map(|(nid, tasks, more)| (nid, (tasks, more)))
        .collect();
    let activity_map: std::collections::HashMap<NodeId, chrono::DateTime<chrono::Utc>> =
        activity_result?.into_iter().collect();
    let area_map: std::collections::HashMap<NodeId, (NodeId, String)> = area_result?
        .into_iter()
        .map(|(project, area_id, area_title)| (project, (area_id, area_title)))
        .collect();

    let empty = TaskCounts {
        open: 0,
        in_progress: 0,
        done: 0,
        cancelled: 0,
    };
    let mut entries: Vec<ProjectDashboardEntry> = projects
        .into_iter()
        .map(|n| {
            let node_status = n.status.as_str().to_string();
            let task_counts = counts_map
                .get(&n.id)
                .cloned()
                .unwrap_or_else(|| empty.clone());
            let status_section = n.body.as_deref().and_then(|b| extract_section(b, "status"));
            let (open_tasks, has_more_tasks) = open_map.get(&n.id).cloned().unwrap_or_default();
            let last_activity_at = activity_map
                .get(&n.id)
                .copied()
                .map_or(n.updated_at, |t| t.max(n.updated_at));
            let (area_id, area_title) = match area_map.get(&n.id) {
                Some((aid, atitle)) => (Some(*aid), Some(atitle.clone())),
                None => (None, None),
            };
            ProjectDashboardEntry {
                node_id: n.id,
                title: n.title,
                node_status,
                task_counts,
                status_section,
                project_status: n.project_status,
                open_tasks,
                has_more_tasks,
                last_activity_at,
                pinned: n.pinned,
                area_id,
                area_title,
            }
        })
        .collect();

    // Pinned projects float to the top within their group; among
    // unpinned (or among pinned), most recently active first — keeps
    // the 2–3 projects the user is currently working on near the top
    // of each Area.
    entries.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.last_activity_at.cmp(&a.last_activity_at))
    });

    Ok(Json(entries))
}

#[derive(Deserialize)]
struct DashboardActivityQuery {
    /// RFC 3339 lower bound — only return entries `created_at >= since`.
    /// Defaults to 48h ago if absent.
    since: Option<chrono::DateTime<chrono::Utc>>,
    /// Cap on returned rows; defaults to 50, max 200.
    limit: Option<i64>,
}

/// `GET /api/dashboard/activity?since=<rfc3339>&limit=<n>` — recent
/// activity entries for the caller's nodes, joined with each node's
/// title.  Powers the v2.9.0 dashboard recap.
async fn dashboard_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<DashboardActivityQuery>,
) -> Result<Json<Vec<common::activity::RecentActivityEntry>>, ApiError> {
    let since = params
        .since
        .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::hours(48));
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let entries = state
        .activity
        .list_recent_for_owner(&claims.sub, since, limit)
        .await?;
    Ok(Json(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::task::RecurrenceRule;

    fn task(status: TaskStatus, recurrence: Option<RecurrenceRule>) -> Task {
        Task {
            id: TaskId(Uuid::new_v4()),
            node_id: None,
            owner_id: "series-owner".to_string(),
            title: "water plants".to_string(),
            status,
            priority: common::task::TaskPriority::Medium,
            focus_date: NaiveDate::from_ymd_opt(2026, 3, 9),
            due_date: NaiveDate::from_ymd_opt(2026, 3, 10),
            recurrence,
            sort_order: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 12).unwrap()
    }

    #[test]
    fn the_next_occurrence_follows_the_task_to_its_node_after_the_update() {
        // #216: complete and move in one request, and the series moves too.
        let pre = task(TaskStatus::Open, Some(RecurrenceRule::Weekly));
        let SeriesEffect::ScheduleNext { req, .. } =
            series_effect(&pre, Some(&TaskStatus::Done), today())
        else {
            panic!("expected ScheduleNext");
        };
        assert_eq!(req.node_id, None, "built from the task before the update");
        let project = NodeId(Uuid::from_u128(7));
        assert_eq!(
            scheduled_on(req.clone(), Some(project)).node_id,
            Some(project)
        );
        assert_eq!(scheduled_on(req, None).node_id, None);
    }

    #[test]
    fn completing_recurring_task_schedules_next_for_series_owner() {
        let pre = task(TaskStatus::Open, Some(RecurrenceRule::Weekly));
        let SeriesEffect::ScheduleNext { owner_id, req } =
            series_effect(&pre, Some(&TaskStatus::Done), today())
        else {
            panic!("expected ScheduleNext");
        };
        // Regression (security): the occurrence used to be created for the
        // *caller*, so a node editor completing someone else's recurring task
        // took ownership of the series.
        assert_eq!(owner_id, "series-owner");
        assert_eq!(req.status, Some(TaskStatus::Open));
        assert_eq!(req.focus_date, NaiveDate::from_ymd_opt(2026, 3, 16));
        assert_eq!(req.due_date, NaiveDate::from_ymd_opt(2026, 3, 17));
        assert_eq!(req.recurrence, Some(RecurrenceRule::Weekly));
    }

    #[test]
    fn completing_from_in_progress_also_schedules() {
        let pre = task(TaskStatus::InProgress, Some(RecurrenceRule::Daily));
        assert!(matches!(
            series_effect(&pre, Some(&TaskStatus::Done), today()),
            SeriesEffect::ScheduleNext { .. }
        ));
    }

    #[test]
    fn completing_non_recurring_or_already_terminal_is_unchanged() {
        let plain = task(TaskStatus::Open, None);
        assert!(matches!(
            series_effect(&plain, Some(&TaskStatus::Done), today()),
            SeriesEffect::Unchanged
        ));
        for from in [TaskStatus::Done, TaskStatus::Cancelled] {
            let pre = task(from, Some(RecurrenceRule::Daily));
            assert!(matches!(
                series_effect(&pre, Some(&TaskStatus::Done), today()),
                SeriesEffect::Unchanged
            ));
        }
    }

    #[test]
    fn reopening_completed_task_retracts_next() {
        for to in [TaskStatus::Open, TaskStatus::InProgress] {
            let pre = task(TaskStatus::Done, Some(RecurrenceRule::Daily));
            assert!(matches!(
                series_effect(&pre, Some(&to), today()),
                SeriesEffect::RetractNext
            ));
        }
        // Rule cleared after completion: the scheduled occurrence still exists.
        let pre = task(TaskStatus::Done, None);
        assert!(matches!(
            series_effect(&pre, Some(&TaskStatus::Open), today()),
            SeriesEffect::RetractNext
        ));
    }

    #[test]
    fn non_status_edits_and_cancelling_are_unchanged() {
        let pre = task(TaskStatus::Done, Some(RecurrenceRule::Daily));
        assert!(matches!(
            series_effect(&pre, None, today()),
            SeriesEffect::Unchanged
        ));
        assert!(matches!(
            series_effect(&pre, Some(&TaskStatus::Cancelled), today()),
            SeriesEffect::Unchanged
        ));
    }
}
