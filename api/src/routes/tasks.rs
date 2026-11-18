use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, patch, post, put},
};
use chrono::NaiveDate;
use common::{
    auth::AuthClaims,
    id::{NodeId, TaskId},
    node::{NodeListParams, NodeType},
    ontology::Capability,
    task::{
        CreateTaskRequest, ListedTask, MyDayTask, ProjectDashboardEntry, ReorderTasksRequest, Task,
        TaskCounts, UpdateTaskRequest,
    },
    task_dependency::{AddDependencyRequest, TaskDependencies, TaskLink, refused_outright},
    webhook::{EVENT_TASK_CREATED, EVENT_TASK_DELETED, EVENT_TASK_UPDATED},
};
use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::permissions::{
        can_view, missing, or_missing, or_missing_row, require_editor, require_resource_owner,
        require_viewer, subject_scope,
    },
    error::ApiError,
    routes::capability::require_capability,
    state::AppState,
    webhook_dispatch::dispatch_task_event,
};

/// Mounts under `/nodes/:node_id/tasks` and `/tasks`.
pub fn node_task_router() -> Router<AppState> {
    Router::new().route("/", get(list_tasks).post(create_task))
}

pub fn task_router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_task_named_in_body))
        .route("/all", get(list_all_open))
        .route("/{id}", patch(update_task).delete(delete_task))
        .route("/{id}/restore", post(restore_task))
        .route(
            "/{id}/dependencies",
            get(list_dependencies).post(add_dependency),
        )
        .route("/{id}/dependencies/candidates", get(dependency_candidates))
        .route(
            "/{id}/dependencies/not-shown",
            delete(remove_dependencies_not_shown),
        )
        .route("/{id}/dependencies/{other}", delete(remove_dependency))
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

/// Who may change a task: an editor of its node (or an admin). One rule
/// for update, delete and restore (#281, 3.3; it was written out three
/// times), and the one `TaskRepo::reorder` applies in SQL.
/// A task always has a node (S5c), so there is no other case: until then
/// a task on no node was its owner's to change.
///
/// SECURITY (owner decision, 2026-11-10): owning a task on a node is not a
/// way in. The owner used to pass whatever their rights on the node, so
/// someone whose share was revoked could still change or delete the tasks
/// they had created there. Revoking a share now ends that: the task stays
/// with the node.
async fn require_task_editor(
    state: &AppState,
    claims: &AuthClaims,
    task: &Task,
) -> Result<(), ApiError> {
    let node = task.node_id;
    // A refusal is a 403 only for someone who can see the task's node;
    // for anyone else the task reads as missing (`permissions::or_missing`).
    let check = require_editor(state.permissions.as_ref(), claims, node).await;
    let may_see = check.is_ok() || can_view(state.permissions.as_ref(), claims, node).await?;
    or_missing(check, may_see, TASK)
}

/// What a task that isn't there, or isn't the caller's to know of, is called.
const TASK: &str = "task";

/// What someone is told who may edit a task and may not take it to another
/// person's Project.
const MOVE_OUT: &str = "only the owner of a task's Project can move it to another person's Project";

/// A task moves between Projects of **one account** on edit rights alone.
/// To a Project that belongs to another account, only the owner of the
/// Project it leaves may move it (or an admin).
///
/// SECURITY (owner decision, 2026-11-18; review of #349). Edit rights on
/// both Projects used to be enough, so someone a Project was shared with
/// could move its tasks to a Project of their own. Two things followed:
/// they kept the task after the share ended (the ruling that a task
/// follows its node's rights, 2026-11-10), and as the owner of the task's
/// new Project they were told how many of its prerequisites they can't
/// see (`TaskDependencyRepo::not_shown`).
///
/// "The owner" is the account the node belongs to (`nodes.owner_id`), as
/// in the rule about links not shown: someone else holding an Owner grant
/// does not pass. The caller has already been checked as an editor of
/// both nodes, so the refusal tells them nothing they can't see.
async fn require_owner_to_move_out(
    state: &AppState,
    claims: &AuthClaims,
    from: NodeId,
    to: NodeId,
) -> Result<(), ApiError> {
    let source = state.nodes.get(from).await?;
    let dest = state.nodes.get(to).await?;
    if source.owner_id == dest.owner_id {
        return Ok(());
    }
    require_resource_owner(claims, &source.owner_id)
        .map_err(|_| ApiError::Forbidden(MOVE_OUT.to_string()))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn list_tasks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<ListedTask>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(node_id)).await?;
    let tasks = state
        .tasks
        .list_for_node(NodeId(node_id), subject_scope(&claims))
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
        .create(NodeId(node_id), &claims.sub, req)
        .await?;
    dispatch_task_event(
        state.webhooks.clone(),
        state.nodes.clone(),
        EVENT_TASK_CREATED,
        task.node_id,
        &claims.sub,
        &task.owner_id,
    );
    Ok((StatusCode::CREATED, Json(task)))
}

/// POST /tasks — create a task on the Project its body names
/// (`POST /nodes/{id}/tasks` takes it from the path, and is what the app's
/// pages use; this one stays for a tab left open across 3.25, whose
/// calendar composer posts here, and for other API clients).
///
/// S5c (ONTOLOGY §9.3): a body that names no Project is refused. It used
/// to make a task on no node (the old Inbox); the loose thing is a capture
/// now (§9.1). The refusal is decided from the request alone, before
/// anything is looked up.
async fn create_task_named_in_body(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let Some(node_id) = req.node_id else {
        return Err(ApiError::Validation(
            common::task::A_TASK_NEEDS_A_PROJECT.to_string(),
        ));
    };
    require_editor(state.permissions.as_ref(), &claims, node_id).await?;
    require_capability(&state, node_id, Capability::Tasks).await?;
    let task = state.tasks.create(node_id, &claims.sub, req).await?;
    dispatch_task_event(
        state.webhooks.clone(),
        state.nodes.clone(),
        EVENT_TASK_CREATED,
        task.node_id,
        &claims.sub,
        &task.owner_id,
    );
    Ok((StatusCode::CREATED, Json(task)))
}

/// Backlog feed for the My Day Kanban — every open task across every node,
/// joined with parent node title.  See `TaskRepo::list_open_for_owner` for
/// the sort contract.
async fn list_all_open(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<MyDayTask>>, ApiError> {
    let tasks = state
        .tasks
        .list_open_for_owner(&claims.sub, subject_scope(&claims))
        .await?;
    Ok(Json(tasks))
}

async fn update_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<Task>, ApiError> {
    // The request's own shape first: it says nothing about any task.
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    // Fetch the task to verify ownership / node permission.
    let existing = state
        .tasks
        .get(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &existing).await?;

    // SECURITY: moving a task onto a node (node_id: Some(Some(_))) requires
    // editor access to the DESTINATION — the check above only covers the
    // task's current node. Mirrors `create_task_named_in_body`. Taking a
    // task off its node (`Some(None)`) never gets this far: `validate()`
    // above refuses it (S5c).
    if let Some(Some(dest)) = req.node_id {
        require_editor(state.permissions.as_ref(), &claims, dest).await?;
        // §3.2 (#216): only onto a Project. Staying on the current node is
        // not a move, so a task already on a non-Project stays editable.
        if existing.node_id != dest {
            require_capability(&state, dest, Capability::Tasks).await?;
            require_owner_to_move_out(&state, &claims, existing.node_id, dest).await?;
        }
    }

    let updated = state.tasks.update(TaskId(id), req).await?;

    dispatch_task_event(
        state.webhooks.clone(),
        state.nodes.clone(),
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
    let existing = state
        .tasks
        .get(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &existing).await?;
    let (owner_id, node_id) = (existing.owner_id.clone(), existing.node_id);
    state.tasks.delete(TaskId(id)).await?;
    dispatch_task_event(
        state.webhooks.clone(),
        state.nodes.clone(),
        EVENT_TASK_DELETED,
        node_id,
        &claims.sub,
        &owner_id,
    );
    Ok(StatusCode::NO_CONTENT)
}

/// POST /tasks/:id/restore — un-delete a soft-deleted task. Backs the undo
/// toast; authorization mirrors delete (`require_task_editor`: an editor of
/// the task's node, or an admin).
async fn restore_task(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Task>, ApiError> {
    let existing = state
        .tasks
        .get_deleted(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &existing).await?;
    let restored = state.tasks.restore(TaskId(id)).await?;
    dispatch_task_event(
        state.webhooks.clone(),
        state.nodes.clone(),
        EVENT_TASK_UPDATED,
        restored.node_id,
        &claims.sub,
        &restored.owner_id,
    );
    Ok(Json(restored))
}

// ── Dependencies (S5d; ONTOLOGY §9.3, §6.4) ───────────────────────────────────

/// What a link that isn't there, or isn't the caller's to know of, is called.
const DEPENDENCY: &str = "dependency";

/// The longest text the picker's search takes: a title's own limit.
const MAX_CANDIDATE_QUERY: usize = 500;

/// `task`'s dependencies as the caller sees them. A task on a node they
/// can't view reads as missing. `may_edit` is [`require_task_editor`]'s
/// rule read for them, so the page offers add and remove only where the
/// call would pass.
async fn dependencies_for(
    state: &AppState,
    claims: &AuthClaims,
    task: &Task,
) -> Result<TaskDependencies, ApiError> {
    let permissions = state.permissions.as_ref();
    let may_edit = match require_editor(permissions, claims, task.node_id).await {
        Ok(()) => true,
        Err(ApiError::Forbidden(_)) => false,
        Err(e) => return Err(e),
    };
    if !may_edit && !can_view(permissions, claims, task.node_id).await? {
        return Err(missing(TASK));
    }
    let viewer = subject_scope(claims);
    let (blocked_by, blocks) = state.task_dependencies.list(task.id, viewer).await?;
    // SECURITY: how many prerequisites the reader isn't shown is told only
    // to someone who may edit the task (they can remove them); a viewer
    // is told nothing of what they can't see.
    let not_shown = if may_edit {
        state.task_dependencies.not_shown(task.id, viewer).await?
    } else {
        0
    };
    // What the prerequisites wait for in turn (S7b), read as the same
    // reader: a viewer of the task sees the chain too, through the tasks
    // they are shown. It is no part of `not_shown`: those are this
    // task's own links, and an editor of this task has no say over its
    // prerequisites' links.
    let (also, also_truncated) = state.task_dependencies.chain(task.id, viewer).await?;
    Ok(TaskDependencies {
        blocked_by,
        blocks,
        not_shown,
        may_edit,
        also,
        also_truncated,
    })
}

/// GET /tasks/{id}/dependencies — the tasks this one waits for, and the
/// tasks that wait for it. Needs view rights on the task's node. Both
/// lists hold only live tasks on nodes the caller can view.
async fn list_dependencies(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDependencies>, ApiError> {
    let task = state
        .tasks
        .get(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    dependencies_for(&state, &claims, &task).await.map(Json)
}

/// POST /tasks/{id}/dependencies — make the task wait for another.
///
/// The order of the checks is the rule (the edges' order, #224):
/// 1. what the request alone settles (a task waiting for itself);
/// 2. the task, and edit rights on it;
/// 3. the other task, which needs view rights on its node;
/// 4. the structure, in the repo: the limit, no loop, no repeat.
///
/// No webhook is sent, here or on remove: the task itself is unchanged.
///
/// SECURITY: the other task is looked at only after the caller has proved
/// they may edit this one, and it answers the same 404 whether it doesn't
/// exist or sits on a node they can't view. So the route can't be used to
/// test an id.
async fn add_dependency(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<AddDependencyRequest>,
) -> Result<(StatusCode, Json<TaskDependencies>), ApiError> {
    let (task_id, other) = (TaskId(id), req.depends_on);
    if let Some(refusal) = refused_outright(task_id, other) {
        return Err(ApiError::Validation(refusal.message()));
    }
    let task = state
        .tasks
        .get(task_id)
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &task).await?;

    let prerequisite = state.tasks.get(other).await.map_err(or_missing_row(TASK))?;
    if !can_view(state.permissions.as_ref(), &claims, prerequisite.node_id).await? {
        return Err(missing(TASK));
    }

    state
        .task_dependencies
        .add(task_id, other)
        .await
        .map_err(or_missing_row(TASK))?;
    // No webhook: the task itself is unchanged.
    let fresh = dependencies_for(&state, &claims, &task).await?;
    Ok((StatusCode::CREATED, Json(fresh)))
}

/// DELETE /tasks/{id}/dependencies/{other} — stop the task waiting for
/// `other`. Needs edit rights on the task.
///
/// SECURITY: `other` is checked only inside the delete itself: a link is
/// removed only when the caller is shown its other task. No link, a link
/// to a task in the trash and a link to a task they can't see all answer
/// the same 404.
async fn remove_dependency(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((id, other)): Path<(Uuid, Uuid)>,
) -> Result<Json<TaskDependencies>, ApiError> {
    let task = state
        .tasks
        .get(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &task).await?;
    state
        .task_dependencies
        .remove(task.id, TaskId(other), subject_scope(&claims))
        .await
        .map_err(or_missing_row(DEPENDENCY))?;
    dependencies_for(&state, &claims, &task).await.map(Json)
}

/// DELETE /tasks/{id}/dependencies/not-shown — remove every prerequisite
/// of the task that the caller is not shown: one in the trash, or on a
/// node they can't view. Needs edit rights on the task. Answers with the
/// fresh lists, whether or not there was anything to remove.
///
/// SECURITY: such a link is never named, but it still decides which
/// dependencies are refused as loops. Without this, someone could leave
/// one in another person's tasks for good (security review, 2026-11-16).
/// The task's editors are told how many there are (`not_shown`) and take
/// them all out here: the rule a hidden home got (#228).
async fn remove_dependencies_not_shown(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDependencies>, ApiError> {
    let task = state
        .tasks
        .get(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &task).await?;
    state
        .task_dependencies
        .remove_not_shown(task.id, subject_scope(&claims))
        .await?;
    dependencies_for(&state, &claims, &task).await.map(Json)
}

#[derive(Deserialize)]
struct CandidateQuery {
    #[serde(default)]
    q: String,
}

/// GET /tasks/{id}/dependencies/candidates?q= — the tasks the picker
/// offers: still to do, not already a prerequisite, and never one that
/// `add` would refuse as a loop. Needs edit rights on the task.
///
/// SECURITY: bound to the caller's own account, never an admin's
/// "everyone": an admin's picker offers their own tasks and the ones
/// shared with them, not every account's titles.
async fn dependency_candidates(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Query(query): Query<CandidateQuery>,
) -> Result<Json<Vec<TaskLink>>, ApiError> {
    // From the request alone, before any task is looked up. A control
    // character can't be in a title, and a NUL can't be sent to the
    // database at all (it would answer as a 500).
    if query.q.chars().count() > MAX_CANDIDATE_QUERY || query.q.chars().any(char::is_control) {
        return Err(ApiError::Validation(format!(
            "search text is at most {MAX_CANDIDATE_QUERY} characters, with no control characters"
        )));
    }
    let task = state
        .tasks
        .get(TaskId(id))
        .await
        .map_err(or_missing_row(TASK))?;
    require_task_editor(&state, &claims, &task).await?;
    let offered = state
        .task_dependencies
        .candidates(task.id, &claims.sub, &query.q)
        .await?;
    Ok(Json(offered))
}

/// PUT /tasks/reorder — bulk update sort_order for drag-to-reorder in My Day
/// and a node's task list.
async fn reorder_tasks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<ReorderTasksRequest>,
) -> Result<StatusCode, ApiError> {
    let updates: Vec<(TaskId, i32)> = req.tasks.iter().map(|e| (e.id, e.sort_order)).collect();
    // The repo applies the edit rule per task (`require_task_editor`'s)
    // and skips the rest.
    state
        .tasks
        .reorder(&updates, subject_scope(&claims))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /my-day?date=YYYY-MM-DD  (defaults to today UTC if omitted)
async fn my_day(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(q): Query<MyDayQuery>,
) -> Result<Json<Vec<MyDayTask>>, ApiError> {
    let date = q.date.unwrap_or_else(|| chrono::Utc::now().date_naive());
    let tasks = state
        .tasks
        .list_my_day(&claims.sub, subject_scope(&claims), date)
        .await?;
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

    let tasks = state
        .tasks
        .list_by_due_range(&claims.sub, subject_scope(&claims), from, to)
        .await?;
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
        // Active and parked projects (S6a): a complete or cancelled one
        // has left the dashboard. Filtered in SQL, so closed projects
        // don't use up the page.
        project_state: None,
        exclude_closed: true,
        tag_id: None,
        owner_id: Some(claims.sub.clone()),
        subject_id: None,
        page: Some(1),
        per_page: Some(500),
        include_archived: false,
        sort: Default::default(),
    };
    let (projects, _) = state.nodes.list(params).await?;
    let mut entries = dashboard_entries(&state, &claims, projects).await?;

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

/// The dashboard card of each project, in the order given: its task
/// counts, top open tasks, latest activity, Area and Brief. Shared by the
/// dashboard and Focus (#282), so a project reads the same in both.
///
/// SECURITY: the caller must already be allowed to view every node in
/// `projects`. The dashboard passes the caller's own projects; Focus
/// passes rows its query read under the visibility check. Tasks and the
/// Brief follow the node's rights, so a viewer may read them; the Area is
/// looked up as the reader, so a hidden one is never named.
pub(crate) async fn dashboard_entries(
    state: &AppState,
    claims: &AuthClaims,
    projects: Vec<common::node::Node>,
) -> Result<Vec<ProjectDashboardEntry>, ApiError> {
    if projects.is_empty() {
        return Ok(vec![]);
    }

    let node_ids: Vec<NodeId> = projects.iter().map(|n| n.id).collect();

    // Fetch counts, open tasks, per-node task activity, and Area
    // parents (PARA grouping, v2.9.0) in parallel.
    let (counts_result, open_result, activity_result, area_result, brief_result) = tokio::join!(
        state.tasks.counts_for_nodes(&node_ids),
        state
            .tasks
            .list_open_for_nodes(&node_ids, DASHBOARD_TASK_LIMIT),
        state.tasks.max_task_updated_for_nodes(&node_ids),
        state.nodes.area_for_nodes(&node_ids, subject_scope(claims)),
        // Briefs (#282): the caller may view every one of these nodes.
        state.briefs.for_nodes(&node_ids),
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

    let mut brief_map: std::collections::HashMap<NodeId, common::brief::ProjectBrief> =
        brief_result?.into_iter().map(|b| (b.node_id, b)).collect();

    // Blockers (S7b): each project's own unmet dependencies, and how
    // many of its tasks say "Blocked". SECURITY: both counted as the
    // reader (a Focus card may be someone else's Project): a dependency
    // or a prerequisite they can't see is not counted. Read after the
    // five above, not beside them: the pool is small, and the dashboard
    // and Focus load together.
    let viewer = subject_scope(claims);
    let (blocked_result, blocked_tasks_result) = tokio::join!(
        state.nodes.blocked_by_for_nodes(&node_ids, viewer),
        state
            .task_dependencies
            .blocked_tasks_for_nodes(&node_ids, viewer),
    );
    let blocked_map: std::collections::HashMap<NodeId, u32> = blocked_result?.into_iter().collect();
    let blocked_tasks_map: std::collections::HashMap<NodeId, u32> =
        blocked_tasks_result?.into_iter().collect();

    let empty = TaskCounts {
        open: 0,
        waiting: 0,
        done: 0,
        cancelled: 0,
    };
    Ok(projects
        .into_iter()
        .map(|n| {
            let node_status = n.status.as_str().to_string();
            let task_counts = counts_map
                .get(&n.id)
                .cloned()
                .unwrap_or_else(|| empty.clone());
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
                project_status: n.project_status,
                project_state: n.project_state,
                project_state_at: n.project_state_at,
                open_tasks,
                has_more_tasks,
                last_activity_at,
                pinned: n.pinned,
                area_id,
                area_title,
                brief: brief_map.remove(&n.id),
                blocked_by: blocked_map.get(&n.id).copied().unwrap_or(0),
                blocked_tasks: blocked_tasks_map.get(&n.id).copied().unwrap_or(0),
            }
        })
        .collect())
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
    // SECURITY: these are the caller's own nodes, but a cascade's entry
    // can refer to a sub-Project of someone else's that they can't view
    // (S6b). Shown as the reader may see it, as a node's History is.
    let entries =
        crate::routes::nodes::present_for(&state, &claims, entries, |e| &mut e.entry).await?;
    Ok(Json(entries))
}
