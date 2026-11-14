use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, TaskId},
    project_state::ProjectState,
    task::{
        CreateTaskRequest, MyDayTask, RecurrenceRule, Task, TaskCounts, TaskPriority, TaskStatus,
        TaskSummary, UpdateTaskRequest,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait TaskRepo: Send + Sync {
    /// Create a task. `node_id = None` produces a standalone (inbox) task.
    async fn create(
        &self,
        node_id: Option<NodeId>,
        owner_id: &str,
        req: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError>;

    async fn list_for_node(
        &self,
        node_id: NodeId,
        owner_id: &str,
    ) -> Result<Vec<Task>, EmberTroveError>;

    /// Create the next occurrence of a recurring task, linked to `parent` —
    /// the instance whose completion scheduled it — so that reopening
    /// `parent` can retract it ([`TaskRepo::retract_occurrence`]).
    ///
    /// A parent has at most one live occurrence (#281, 1.8). Two
    /// completions that arrive together (a double click, two devices) both
    /// read the task as open, and both call this: the parent row is locked,
    /// and the second caller gets the occurrence the first one made.
    async fn create_occurrence(
        &self,
        parent: TaskId,
        owner_id: &str,
        req: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError>;

    /// Soft-delete the occurrence scheduled by completing `parent`, but only
    /// while it is still untouched-open, live, and owned by `owner_id` (an
    /// occurrence already started or finished is the user's work — keep it).
    /// Returns the number of rows retracted (0 or 1).
    async fn retract_occurrence(
        &self,
        parent: TaskId,
        owner_id: &str,
    ) -> Result<u64, EmberTroveError>;

    async fn get(&self, id: TaskId) -> Result<Task, EmberTroveError>;

    async fn update(&self, id: TaskId, req: UpdateTaskRequest) -> Result<Task, EmberTroveError>;

    /// Soft-delete: stamps `deleted_at`; the row becomes invisible to every
    /// other query but can be brought back with [`TaskRepo::restore`].
    async fn delete(&self, id: TaskId) -> Result<(), EmberTroveError>;

    /// Fetch a tombstoned task (`deleted_at IS NOT NULL`) — used by the
    /// restore endpoint's authorization check. [`TaskRepo::get`] deliberately
    /// excludes tombstones.
    async fn get_deleted(&self, id: TaskId) -> Result<Task, EmberTroveError>;

    /// Un-delete a tombstoned task (clears `deleted_at`).
    async fn restore(&self, id: TaskId) -> Result<Task, EmberTroveError>;

    /// Hard-delete tombstones older than `cutoff`. Returns rows purged.
    async fn purge_deleted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EmberTroveError>;

    /// Tasks the caller has marked for focus on `date` (My Day), with node titles.
    ///
    /// `viewer` scopes the joined `node_title` (#230): it is `NULL` when the
    /// node is no longer visible to that subject (a revoked share), so a
    /// task never shows a node's live title to someone who lost access.
    /// `None` = admin (sees every node); routes pass `subject_scope`.
    async fn list_my_day(
        &self,
        owner_id: &str,
        viewer: Option<&str>,
        date: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError>;

    /// Standalone tasks (node_id IS NULL) owned by `owner_id`.
    async fn list_inbox(&self, owner_id: &str) -> Result<Vec<Task>, EmberTroveError>;

    /// Tasks with a due_date in [from, to] inclusive, ordered by due_date then priority.
    /// `viewer` scopes `node_title` as in [`TaskRepo::list_my_day`].
    async fn list_by_due_range(
        &self,
        owner_id: &str,
        viewer: Option<&str>,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError>;

    /// Aggregated task counts per project node for the dashboard.
    async fn counts_for_nodes(
        &self,
        node_ids: &[NodeId],
    ) -> Result<Vec<(NodeId, TaskCounts)>, EmberTroveError>;

    /// The tasks still to do on the given nodes, open ones before waiting
    /// ones, capped at `limit` per node.
    /// Returns `(node_id, tasks, has_more)` tuples.
    async fn list_open_for_nodes(
        &self,
        node_ids: &[NodeId],
        limit_per_node: i64,
    ) -> Result<Vec<(NodeId, Vec<TaskSummary>, bool)>, EmberTroveError>;

    /// Most recent `updated_at` across tasks for each given node.
    /// Only returns entries for nodes that have at least one task; absent
    /// nodes should fall back to the node's own `updated_at` at the caller.
    async fn max_task_updated_for_nodes(
        &self,
        node_ids: &[NodeId],
    ) -> Result<Vec<(NodeId, DateTime<Utc>)>, EmberTroveError>;

    /// All tasks owned by `owner_id` — used for backup.
    async fn list_all_for_owner(&self, owner_id: &str) -> Result<Vec<Task>, EmberTroveError>;

    /// All open tasks (status NOT IN done/cancelled) owned by `owner_id`,
    /// joined with their parent node title.  Used by the My Day Kanban
    /// backlog zone where the user sees every open task to plan against.
    /// Sorted: due_date ASC NULLS LAST, then priority desc, then created_at ASC.
    /// `viewer` scopes `node_title` as in [`TaskRepo::list_my_day`].
    async fn list_open_for_owner(
        &self,
        owner_id: &str,
        viewer: Option<&str>,
    ) -> Result<Vec<MyDayTask>, EmberTroveError>;

    /// All tasks across all owners — used for full backup.
    async fn list_all(&self) -> Result<Vec<Task>, EmberTroveError>;

    /// Bulk update sort_order for drag-to-reorder; only touches rows owned by `owner_id`.
    /// Sets `sort_order` on the tasks `subject` may edit, by the rule of
    /// `PATCH /tasks/:id`: their own, or one on a node they edit (#281,
    /// 2.5; it was owner-only, so an editor's drag on a shared node's
    /// task was silently dropped). `None` is an admin. Any other id is
    /// skipped without an error, as before.
    async fn reorder(
        &self,
        updates: &[(TaskId, i32)],
        subject: Option<&str>,
    ) -> Result<(), EmberTroveError>;
}

// ── PgTaskRepo ────────────────────────────────────────────────────────────────

pub struct PgTaskRepo {
    pool: PgPool,
}

impl PgTaskRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Shared INSERT behind [`TaskRepo::create`] and
    /// [`TaskRepo::create_occurrence`] (the only caller that sets `parent`).
    async fn insert(
        &self,
        node_id: Option<NodeId>,
        owner_id: &str,
        req: CreateTaskRequest,
        parent: Option<TaskId>,
    ) -> Result<Task, EmberTroveError> {
        insert_task(&self.pool, node_id, owner_id, &req, parent).await
    }
}

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: Uuid,
    node_id: Option<Uuid>,
    owner_id: String,
    title: String,
    status: String,
    priority: String,
    focus_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    recurrence: Option<String>,
    sort_order: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

fn parse_status(s: &str) -> Result<TaskStatus, EmberTroveError> {
    super::parse_wire(s, "task_status", TaskStatus::parse)
}

fn parse_priority(s: &str) -> Result<TaskPriority, EmberTroveError> {
    super::parse_wire(s, "task_priority", TaskPriority::parse)
}

pub fn parse_recurrence(s: &str) -> Result<RecurrenceRule, EmberTroveError> {
    super::parse_wire(s, "recurrence", RecurrenceRule::parse)
}

impl TaskRow {
    fn into_task(self) -> Result<Task, EmberTroveError> {
        let recurrence = self
            .recurrence
            .as_deref()
            .map(parse_recurrence)
            .transpose()?;
        Ok(Task {
            id: TaskId(self.id),
            node_id: self.node_id.map(NodeId),
            owner_id: self.owner_id,
            title: self.title,
            status: parse_status(&self.status)?,
            priority: parse_priority(&self.priority)?,
            focus_date: self.focus_date,
            due_date: self.due_date,
            recurrence,
            sort_order: self.sort_order,
            created_at: self.created_at,
            updated_at: self.updated_at,
            completed_at: self.completed_at,
        })
    }
}

/// For the owner's task feeds, which `LEFT JOIN nodes n` on the node being
/// visible to the reader: keep a task with no node, or whose node they see.
///
/// SECURITY (owner decision, 2026-11-10): a task on a node its owner can no
/// longer see (a revoked share) is not in their feeds. #230 hid only the
/// node's title and kept the task.
const ON_A_NODE_THEY_SEE: &str = "(t.node_id IS NULL OR n.id IS NOT NULL)";

/// A task's priority as an `ORDER BY` rank: high, medium, low. One
/// definition for every task list (#281, 3.5; it was written out five
/// times). `T_` is the same for queries that alias `node_tasks` as `t`.
const PRIORITY_RANK: &str =
    "CASE priority::text WHEN 'high' THEN 0 WHEN 'medium' THEN 1 WHEN 'low' THEN 2 END";
const T_PRIORITY_RANK: &str =
    "CASE t.priority::text WHEN 'high' THEN 0 WHEN 'medium' THEN 1 WHEN 'low' THEN 2 END";

/// The columns a [`MyDayRow`] reads: the task (`t`) and what its feeds
/// show of its node (`n`, joined under the reader's visibility, so both
/// are NULL for a node they can't see; such a task is not listed at all,
/// [`ON_A_NODE_THEY_SEE`]). One list for the three feeds: `FromRow` finds
/// a missing column only when that feed runs.
const FEED_COLS: &str = "t.id, t.node_id, t.owner_id, t.title, \
     t.status::text AS status, t.priority::text AS priority, \
     t.focus_date, t.due_date, t.recurrence, t.sort_order, \
     t.created_at, t.updated_at, t.completed_at, \
     n.title AS node_title, n.project_state AS project_state";

/// SQL for `Task::asks_for_attention` in a feed query (`t` the task, `n`
/// its visible node): the task can be acted on and its project, if it is
/// on one, asks for attention. Built from the two rules themselves
/// (`TaskStatus::is_actionable`, `ProjectState::asks_for_attention`), so
/// the SQL can't name a different set; a Postgres test compares the list
/// it gives with the Rust rule for every combination. The words are the
/// enums' own wire strings, never text from a request.
fn asks_for_attention() -> String {
    let quoted = |words: Vec<&'static str>| {
        words
            .iter()
            .map(|w| format!("'{w}'"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let actionable = quoted(
        TaskStatus::ALL
            .iter()
            .filter(|s| s.is_actionable())
            .map(TaskStatus::as_str)
            .collect(),
    );
    let asking = quoted(
        ProjectState::ALL
            .iter()
            .filter(|s| s.asks_for_attention())
            .map(ProjectState::as_str)
            .collect(),
    );
    // No state: a task on no node, or on a node that is not a Project.
    format!(
        "(t.status::text IN ({actionable}) \
          AND (n.project_state IS NULL OR n.project_state IN ({asking})))"
    )
}

const SELECT_COLS: &str = r#"
    id, node_id, owner_id, title,
    status::text    AS status,
    priority::text  AS priority,
    focus_date, due_date, recurrence, sort_order, created_at, updated_at, completed_at
"#;

/// The completion date a status write leaves behind (ONTOLOGY §9.3; S5a,
/// #314), as the right-hand side of `completed_at = …` in an `UPDATE` whose
/// new status, or NULL for "no change", is bound at `$3`:
/// - no status in the request: untouched;
/// - any state but done: cleared (a reopened or cancelled task has none);
/// - done when it already was: untouched, so a task completed before the
///   column existed keeps its empty date (never made up);
/// - becoming done: now.
///
/// It lives in the statement that sets the status, so the two can't part.
/// The database has only the other half as a CHECK (migration 050 says why).
const COMPLETED_AT_AFTER: &str = "CASE \
    WHEN $3::task_status IS NULL THEN completed_at \
    WHEN $3::task_status <> 'done' THEN NULL \
    WHEN status = 'done' THEN completed_at \
    ELSE now() END";

#[derive(sqlx::FromRow)]
struct MyDayRow {
    id: Uuid,
    node_id: Option<Uuid>,
    owner_id: String,
    title: String,
    status: String,
    priority: String,
    focus_date: Option<NaiveDate>,
    due_date: Option<NaiveDate>,
    recurrence: Option<String>,
    sort_order: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    node_title: Option<String>,
    project_state: Option<String>,
}

fn my_day_row_to_task(r: MyDayRow) -> Result<MyDayTask, EmberTroveError> {
    let task = TaskRow {
        id: r.id,
        node_id: r.node_id,
        owner_id: r.owner_id,
        title: r.title,
        status: r.status,
        priority: r.priority,
        focus_date: r.focus_date,
        due_date: r.due_date,
        recurrence: r.recurrence,
        sort_order: r.sort_order,
        created_at: r.created_at,
        updated_at: r.updated_at,
        completed_at: r.completed_at,
    }
    .into_task()?;
    let project_state = r
        .project_state
        .as_deref()
        .map(|s| super::parse_wire(s, "project_state", ProjectState::parse))
        .transpose()?;
    Ok(MyDayTask {
        task,
        node_title: r.node_title,
        project_state,
    })
}

/// The task states that are still to do (open, waiting): what a cascade
/// cancels, and what "open under a closed project" means. From the enum,
/// so the SQL can't name a different set.
pub(crate) fn still_to_do() -> Vec<&'static str> {
    TaskStatus::ALL
        .iter()
        .filter(|s| !s.is_terminal())
        .map(TaskStatus::as_str)
        .collect()
}

/// Cancel the tasks `ids` that are still to do, in one statement on the
/// caller's transaction: the cascade's write (ONTOLOGY §4.1; S6b). The
/// caller has decided which tasks (the plan, under its locks) and that
/// the actor may: this checks no rights.
///
/// The status goes through [`COMPLETED_AT_AFTER`] like every status
/// write, so a cancelled task never has a completion date. Cancelling
/// schedules nothing and retracts nothing (`series_effect`: only a move
/// to Done does). The tasks trigger moves `updated_at`: it is a change.
/// Returns how many it cancelled.
pub(crate) async fn cancel_open_on(
    conn: &mut sqlx::PgConnection,
    ids: &[Uuid],
) -> Result<u64, EmberTroveError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let done = sqlx::query(&format!(
        "UPDATE node_tasks \
         SET status = $3::task_status, completed_at = {COMPLETED_AT_AFTER} \
         WHERE id = ANY($1::uuid[]) AND deleted_at IS NULL AND status::text = ANY($2::text[])"
    ))
    .bind(ids)
    .bind(still_to_do())
    .bind(TaskStatus::Cancelled.as_str())
    .execute(conn)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("cancel tasks failed: {e}")))?;
    Ok(done.rows_affected())
}

/// Insert a task (`TaskRepo::create`), on any executor, so capture
/// conversion (#234) can do it inside its transaction.
pub(crate) async fn insert_task<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    node_id: Option<NodeId>,
    owner_id: &str,
    req: &CreateTaskRequest,
    parent: Option<TaskId>,
) -> Result<Task, EmberTroveError> {
    // node_id from path param takes precedence; fall back to body field.
    let resolved_node_id = node_id.or(req.node_id).map(|n| n.0);
    let status = req.status.as_ref().map_or("open", |s| s.as_str());
    let priority = req.priority.as_ref().map_or("medium", |p| p.as_str());
    let recurrence = req.recurrence.as_ref().map(|r| r.as_str().to_string());

    let row = sqlx::query_as::<_, TaskRow>(&format!(
        r#"
        INSERT INTO node_tasks
            (node_id, owner_id, title, status, priority, focus_date, due_date, recurrence,
             recurrence_parent_id, completed_at)
        VALUES ($1, $2, $3, $4::task_status, $5::task_priority, $6, $7, $8, $9,
                -- A task created done is completed now (S5a).
                CASE WHEN $4::task_status = 'done' THEN now() END)
        RETURNING {SELECT_COLS}
        "#
    ))
    .bind(resolved_node_id)
    .bind(owner_id)
    .bind(&req.title)
    .bind(status)
    .bind(priority)
    .bind(req.focus_date)
    .bind(req.due_date)
    .bind(recurrence)
    .bind(parent.map(|p| p.0))
    .fetch_one(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("create task failed: {e}")))?;

    row.into_task()
}

/// Every live task, all owners. A free function so the backup can run it on its one
/// snapshot transaction (`backup::read_snapshot`).
pub(crate) async fn list_all_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<Task>, EmberTroveError> {
    let rows = sqlx::query_as::<_, TaskRow>(&format!(
        r#"
        SELECT {SELECT_COLS}
        FROM node_tasks
        WHERE deleted_at IS NULL
        ORDER BY created_at ASC
        "#
    ))
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list_all tasks failed: {e}")))?;

    rows.into_iter().map(TaskRow::into_task).collect()
}

/// Which live task was scheduled by completing which (migration 032), for
/// the backup (`backup::read_snapshot`). The `Task` DTO doesn't carry the
/// link, so it travels beside the tasks. Only links between two live tasks:
/// a link to a deleted task would name a row the archive doesn't hold.
pub(crate) async fn recurrence_links_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<common::backup::RecurrenceLink>, EmberTroveError> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT t.id, t.recurrence_parent_id
         FROM node_tasks t
         JOIN node_tasks p ON p.id = t.recurrence_parent_id
         WHERE t.deleted_at IS NULL AND p.deleted_at IS NULL
         ORDER BY t.created_at, t.id",
    )
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list recurrence links failed: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|(task, parent)| common::backup::RecurrenceLink {
            task_id: TaskId(task),
            parent_id: TaskId(parent),
        })
        .collect())
}

#[async_trait]
impl TaskRepo for PgTaskRepo {
    async fn create(
        &self,
        node_id: Option<NodeId>,
        owner_id: &str,
        req: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError> {
        self.insert(node_id, owner_id, req, None).await
    }

    async fn create_occurrence(
        &self,
        parent: TaskId,
        owner_id: &str,
        req: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError> {
        let db = |what: &'static str| {
            move |e: sqlx::Error| EmberTroveError::Internal(format!("{what} failed: {e}"))
        };
        let mut tx = self.pool.begin().await.map_err(db("begin occurrence"))?;
        // The lock that makes the check below and the insert one step.
        let locked: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM node_tasks WHERE id = $1 FOR UPDATE")
                .bind(parent.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db("lock occurrence parent"))?;
        if locked.is_none() {
            return Err(EmberTroveError::NotFound(format!(
                "task {parent} not found"
            )));
        }
        let existing = sqlx::query_as::<_, TaskRow>(&format!(
            "SELECT {SELECT_COLS} FROM node_tasks \
             WHERE recurrence_parent_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at, id LIMIT 1"
        ))
        .bind(parent.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db("find occurrence"))?;
        if let Some(row) = existing {
            return row.into_task();
        }
        let task = insert_task(&mut *tx, None, owner_id, &req, Some(parent)).await?;
        tx.commit().await.map_err(db("commit occurrence"))?;
        Ok(task)
    }

    async fn retract_occurrence(
        &self,
        parent: TaskId,
        owner_id: &str,
    ) -> Result<u64, EmberTroveError> {
        let result = sqlx::query(
            r#"
            UPDATE node_tasks SET deleted_at = now()
            WHERE recurrence_parent_id = $1
              AND owner_id = $2
              AND status = 'open'::task_status
              AND deleted_at IS NULL
            "#,
        )
        .bind(parent.0)
        .bind(owner_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("retract occurrence failed: {e}")))?;
        Ok(result.rows_affected())
    }

    async fn list_for_node(
        &self,
        node_id: NodeId,
        _owner_id: &str,
    ) -> Result<Vec<Task>, EmberTroveError> {
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            SELECT {SELECT_COLS}
            FROM node_tasks
            WHERE node_id = $1
              AND deleted_at IS NULL
            ORDER BY sort_order ASC, created_at ASC
            "#
        ))
        .bind(node_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list tasks failed: {e}")))?;

        rows.into_iter().map(TaskRow::into_task).collect()
    }

    async fn get(&self, id: TaskId) -> Result<Task, EmberTroveError> {
        let row = sqlx::query_as::<_, TaskRow>(&format!(
            r#"SELECT {SELECT_COLS} FROM node_tasks WHERE id = $1 AND deleted_at IS NULL"#
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get task failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("task {id} not found")))?;

        row.into_task()
    }

    async fn update(&self, id: TaskId, req: UpdateTaskRequest) -> Result<Task, EmberTroveError> {
        let status_s = req.status.as_ref().map(|s| s.as_str().to_string());
        let priority_s = req.priority.as_ref().map(|p| p.as_str().to_string());
        let recurrence_update = req.recurrence.is_some();
        let recurrence_val = req
            .recurrence
            .and_then(|r| r)
            .map(|r| r.as_str().to_string());
        let node_id_update = req.node_id.is_some();
        let node_id_val = req.node_id.and_then(|n| n).map(|n| n.0);

        let row = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            UPDATE node_tasks SET
                title      = COALESCE($2, title),
                status     = COALESCE($3::task_status,   status),
                completed_at = {COMPLETED_AT_AFTER},
                priority   = COALESCE($4::task_priority, priority),
                focus_date = CASE WHEN $5  THEN $6  ELSE focus_date END,
                due_date   = CASE WHEN $7  THEN $8  ELSE due_date   END,
                recurrence = CASE WHEN $9  THEN $10 ELSE recurrence END,
                node_id    = CASE WHEN $11 THEN $12 ELSE node_id    END
            WHERE id = $1 AND deleted_at IS NULL
            RETURNING {SELECT_COLS}
            "#
        ))
        .bind(id.0)
        .bind(&req.title)
        .bind(status_s)
        .bind(priority_s)
        .bind(req.focus_date.is_some())
        .bind(req.focus_date.and_then(|d| d))
        .bind(req.due_date.is_some())
        .bind(req.due_date.and_then(|d| d))
        .bind(recurrence_update)
        .bind(recurrence_val)
        .bind(node_id_update)
        .bind(node_id_val)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("update task failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("task {id} not found")))?;

        row.into_task()
    }

    async fn delete(&self, id: TaskId) -> Result<(), EmberTroveError> {
        // Soft delete: tombstone the row so the undo toast can restore it.
        let result = sqlx::query(
            "UPDATE node_tasks SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("delete task failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!("task {id} not found")));
        }
        Ok(())
    }

    async fn get_deleted(&self, id: TaskId) -> Result<Task, EmberTroveError> {
        let row = sqlx::query_as::<_, TaskRow>(&format!(
            r#"SELECT {SELECT_COLS} FROM node_tasks WHERE id = $1 AND deleted_at IS NOT NULL"#
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get deleted task failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("deleted task {id} not found")))?;

        row.into_task()
    }

    async fn restore(&self, id: TaskId) -> Result<Task, EmberTroveError> {
        let row = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            UPDATE node_tasks SET deleted_at = NULL
            WHERE id = $1 AND deleted_at IS NOT NULL
            RETURNING {SELECT_COLS}
            "#
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("restore task failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("deleted task {id} not found")))?;

        row.into_task()
    }

    async fn purge_deleted_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EmberTroveError> {
        let result =
            sqlx::query("DELETE FROM node_tasks WHERE deleted_at IS NOT NULL AND deleted_at < $1")
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(|e| EmberTroveError::Internal(format!("purge tasks failed: {e}")))?;
        Ok(result.rows_affected())
    }

    async fn list_my_day(
        &self,
        owner_id: &str,
        viewer: Option<&str>,
        date: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError> {
        let rows = sqlx::query_as::<_, MyDayRow>(&format!(
            r#"
            SELECT
                {FEED_COLS}
            FROM node_tasks t
            LEFT JOIN nodes n ON n.id = t.node_id AND {visible}
            WHERE t.owner_id = $1
              AND t.deleted_at IS NULL
              AND {on_a_node_they_see}
              AND t.focus_date IS NOT NULL
              AND t.focus_date <= $2
              -- `Task::in_my_day`: focused that day, or carried over from
              -- an earlier one while it still asks for attention. A
              -- waiting task doesn't carry over (S5a), nor one whose
              -- project is parked or closed (S6a).
              AND (
                  t.focus_date = $2
                  OR {asks_for_attention}
              )
            ORDER BY
                t.sort_order ASC,
                {T_PRIORITY_RANK},
                t.node_id NULLS FIRST,
                t.created_at
            "#,
            visible = super::visible_to("n", 3),
            on_a_node_they_see = ON_A_NODE_THEY_SEE,
            asks_for_attention = asks_for_attention(),
        ))
        .bind(owner_id)
        .bind(date)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list my day failed: {e}")))?;

        rows.into_iter().map(my_day_row_to_task).collect()
    }

    async fn list_inbox(&self, owner_id: &str) -> Result<Vec<Task>, EmberTroveError> {
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            SELECT {SELECT_COLS}
            FROM node_tasks
            WHERE owner_id = $1
              AND deleted_at IS NULL
              AND node_id IS NULL
            ORDER BY
                sort_order ASC,
                {PRIORITY_RANK},
                created_at ASC
            "#
        ))
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list inbox failed: {e}")))?;

        rows.into_iter().map(TaskRow::into_task).collect()
    }

    async fn list_by_due_range(
        &self,
        owner_id: &str,
        viewer: Option<&str>,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError> {
        let rows = sqlx::query_as::<_, MyDayRow>(&format!(
            r#"
            SELECT
                {FEED_COLS}
            FROM node_tasks t
            LEFT JOIN nodes n ON n.id = t.node_id AND {visible}
            WHERE t.owner_id = $1
              AND t.deleted_at IS NULL
              AND {on_a_node_they_see}
              AND t.due_date >= $2
              AND t.due_date <= $3
            ORDER BY t.due_date,
                {T_PRIORITY_RANK},
                t.created_at
            "#,
            visible = super::visible_to("n", 4),
            on_a_node_they_see = ON_A_NODE_THEY_SEE,
        ))
        .bind(owner_id)
        .bind(from)
        .bind(to)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list by due range failed: {e}")))?;

        rows.into_iter().map(my_day_row_to_task).collect()
    }

    async fn list_open_for_owner(
        &self,
        owner_id: &str,
        viewer: Option<&str>,
    ) -> Result<Vec<MyDayTask>, EmberTroveError> {
        // NULLS LAST on due_date so dated tasks float to the top of the
        // backlog; undated open tasks settle at the bottom.  Priority desc
        // (high → low) breaks ties.  created_at ASC keeps order stable
        // across re-fetches for items with no due date.
        let rows = sqlx::query_as::<_, MyDayRow>(&format!(
            r#"
            SELECT
                {FEED_COLS}
            FROM node_tasks t
            LEFT JOIN nodes n ON n.id = t.node_id AND {visible}
            WHERE t.owner_id = $1
              AND t.deleted_at IS NULL
              AND {on_a_node_they_see}
              AND t.status::text NOT IN ('done', 'cancelled')
            ORDER BY
                t.due_date ASC NULLS LAST,
                {T_PRIORITY_RANK},
                t.created_at ASC
            "#,
            visible = super::visible_to("n", 2),
            on_a_node_they_see = ON_A_NODE_THEY_SEE,
        ))
        .bind(owner_id)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list open for owner failed: {e}")))?;

        rows.into_iter().map(my_day_row_to_task).collect()
    }

    async fn counts_for_nodes(
        &self,
        node_ids: &[NodeId],
    ) -> Result<Vec<(NodeId, TaskCounts)>, EmberTroveError> {
        if node_ids.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<Uuid> = node_ids.iter().map(|n| n.0).collect();

        #[derive(sqlx::FromRow)]
        struct CountRow {
            node_id: Uuid,
            open: i64,
            waiting: i64,
            done: i64,
            cancelled: i64,
        }

        let rows = sqlx::query_as::<_, CountRow>(
            r#"
            SELECT
                node_id,
                COUNT(*) FILTER (WHERE status = 'open')        AS open,
                COUNT(*) FILTER (WHERE status = 'waiting')     AS waiting,
                COUNT(*) FILTER (WHERE status = 'done')        AS done,
                COUNT(*) FILTER (WHERE status = 'cancelled')   AS cancelled
            FROM node_tasks
            WHERE node_id = ANY($1)
              AND deleted_at IS NULL
            GROUP BY node_id
            "#,
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("task counts failed: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    NodeId(r.node_id),
                    TaskCounts {
                        open: r.open as u32,
                        waiting: r.waiting as u32,
                        done: r.done as u32,
                        cancelled: r.cancelled as u32,
                    },
                )
            })
            .collect())
    }

    async fn list_open_for_nodes(
        &self,
        node_ids: &[NodeId],
        limit_per_node: i64,
    ) -> Result<Vec<(NodeId, Vec<TaskSummary>, bool)>, EmberTroveError> {
        if node_ids.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<Uuid> = node_ids.iter().map(|n| n.0).collect();

        #[derive(sqlx::FromRow)]
        struct SummaryRow {
            node_id: Uuid,
            id: Uuid,
            title: String,
            status: String,
            priority: String,
            due_date: Option<NaiveDate>,
            // Window-function rank: filtered in the SQL `WHERE rn <= $n`, so sqlx
            // must map the column, but Rust never reads it after deserialization.
            #[allow(dead_code)]
            rn: i64,
        }

        // Use a window function to rank tasks per node and fetch limit+1 to detect "has more".
        let rows = sqlx::query_as::<_, SummaryRow>(&format!(
            r#"
            SELECT node_id, id, title, status::text AS status, priority::text AS priority,
                   due_date, rn
            FROM (
                SELECT node_id, id, title, status, priority, due_date,
                       ROW_NUMBER() OVER (
                           PARTITION BY node_id
                           ORDER BY
                               -- What can be acted on leads; what waits
                               -- on someone else follows (S5a).
                               (status = 'waiting'),
                               {PRIORITY_RANK},
                               due_date NULLS LAST,
                               sort_order ASC,
                               created_at ASC
                       ) AS rn
                FROM node_tasks
                WHERE node_id = ANY($1)
                  AND deleted_at IS NULL
                  AND status::text IN ('open', 'waiting')
            ) ranked
            WHERE rn <= $2
            ORDER BY node_id, rn
            "#
        ))
        .bind(&ids)
        .bind(limit_per_node + 1) // fetch one extra to detect overflow
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_open_for_nodes failed: {e}")))?;

        // Group by node_id.
        let mut map: std::collections::HashMap<Uuid, Vec<SummaryRow>> =
            std::collections::HashMap::new();
        for row in rows {
            map.entry(row.node_id).or_default().push(row);
        }

        let limit_usize = limit_per_node as usize;
        let mut result = Vec::with_capacity(map.len());
        for (&nid, rows) in &mut map {
            let has_more = rows.len() > limit_usize;
            rows.truncate(limit_usize);
            // Propagate unknown status/priority rather than silently falling
            // back to Open/Medium — a silent fallback lets a bad migration
            // render done/cancelled tasks as open on the dashboard.
            let summaries: Vec<TaskSummary> = rows
                .drain(..)
                .map(|r| {
                    Ok(TaskSummary {
                        id: TaskId(r.id),
                        title: r.title,
                        status: parse_status(&r.status)?,
                        priority: parse_priority(&r.priority)?,
                        due_date: r.due_date,
                    })
                })
                .collect::<Result<Vec<_>, EmberTroveError>>()?;
            result.push((NodeId(nid), summaries, has_more));
        }

        Ok(result)
    }

    async fn max_task_updated_for_nodes(
        &self,
        node_ids: &[NodeId],
    ) -> Result<Vec<(NodeId, DateTime<Utc>)>, EmberTroveError> {
        if node_ids.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<Uuid> = node_ids.iter().map(|n| n.0).collect();

        #[derive(sqlx::FromRow)]
        struct MaxRow {
            node_id: Uuid,
            max_updated: DateTime<Utc>,
        }

        let rows = sqlx::query_as::<_, MaxRow>(
            r#"
            SELECT node_id, MAX(updated_at) AS max_updated
            FROM node_tasks
            WHERE node_id = ANY($1)
              AND deleted_at IS NULL
            GROUP BY node_id
            "#,
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            EmberTroveError::Internal(format!("max_task_updated_for_nodes failed: {e}"))
        })?;

        Ok(rows
            .into_iter()
            .map(|r| (NodeId(r.node_id), r.max_updated))
            .collect())
    }

    async fn list_all_for_owner(&self, owner_id: &str) -> Result<Vec<Task>, EmberTroveError> {
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            SELECT {SELECT_COLS}
            FROM node_tasks
            WHERE owner_id = $1
              AND deleted_at IS NULL
            ORDER BY created_at ASC
            "#
        ))
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all_for_owner tasks failed: {e}")))?;

        rows.into_iter().map(TaskRow::into_task).collect()
    }

    async fn list_all(&self) -> Result<Vec<Task>, EmberTroveError> {
        list_all_on(&self.pool).await
    }

    async fn reorder(
        &self,
        updates: &[(TaskId, i32)],
        subject: Option<&str>,
    ) -> Result<(), EmberTroveError> {
        if updates.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = updates.iter().map(|(id, _)| id.0).collect();
        let orders: Vec<i32> = updates.iter().map(|(_, o)| *o).collect();
        let failed =
            |e: sqlx::Error| EmberTroveError::Internal(format!("reorder tasks failed: {e}"));

        // The rows first, in id order, as everything that takes many task
        // rows does (the cascade, the purge). The update below takes them
        // in the order the request lists them, and two writers taking the
        // same rows in different orders can deadlock.
        let mut tx = self.pool.begin().await.map_err(failed)?;
        sqlx::query("SELECT 1 FROM node_tasks WHERE id = ANY($1) ORDER BY id FOR UPDATE")
            .bind(&ids)
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
        sqlx::query(&format!(
            r#"
            UPDATE node_tasks
            SET sort_order = u.sort_order
            FROM (
                SELECT
                    unnest($1::uuid[])  AS id,
                    unnest($2::int4[])  AS sort_order
            ) AS u
            WHERE node_tasks.id = u.id
              AND node_tasks.deleted_at IS NULL
              -- The routes' rule (`require_task_editor`): on a node, its
              -- editors; on none, the task's owner. Owning a task on a
              -- node is not enough (a revoked share). An admin (NULL)
              -- passes here, by this caller's choice.
              AND (
                $3::text IS NULL
                OR (node_tasks.node_id IS NULL AND node_tasks.owner_id = $3::text)
                OR {edits_its_node}
              )
            "#,
            edits_its_node = super::editable_by("node_tasks.node_id", 3),
        ))
        .bind(&ids)
        .bind(&orders)
        .bind(subject)
        .execute(&mut *tx)
        .await
        .map_err(failed)?;
        tx.commit().await.map_err(failed)?;

        Ok(())
    }
}
