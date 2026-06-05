use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, TaskId},
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

    async fn get(&self, id: TaskId) -> Result<Task, EmberTroveError>;

    async fn update(&self, id: TaskId, req: UpdateTaskRequest) -> Result<Task, EmberTroveError>;

    async fn delete(&self, id: TaskId) -> Result<(), EmberTroveError>;

    /// Tasks the caller has marked for focus on `date` (My Day), with node titles.
    async fn list_my_day(
        &self,
        owner_id: &str,
        date: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError>;

    /// Standalone tasks (node_id IS NULL) owned by `owner_id`.
    async fn list_inbox(&self, owner_id: &str) -> Result<Vec<Task>, EmberTroveError>;

    /// Tasks with a due_date in [from, to] inclusive, ordered by due_date then priority.
    async fn list_by_due_range(
        &self,
        owner_id: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError>;

    /// Aggregated task counts per project node for the dashboard.
    async fn counts_for_nodes(
        &self,
        node_ids: &[NodeId],
    ) -> Result<Vec<(NodeId, TaskCounts)>, EmberTroveError>;

    /// Open/in-progress tasks for the given nodes, capped at `limit` per node.
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
    async fn list_open_for_owner(&self, owner_id: &str) -> Result<Vec<MyDayTask>, EmberTroveError>;

    /// All tasks across all owners — used for full backup.
    async fn list_all(&self) -> Result<Vec<Task>, EmberTroveError>;

    /// Bulk update sort_order for drag-to-reorder; only touches rows owned by `owner_id`.
    async fn reorder(
        &self,
        updates: &[(TaskId, i32)],
        owner_id: &str,
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
}

fn parse_status(s: &str) -> Result<TaskStatus, EmberTroveError> {
    match s {
        "open" => Ok(TaskStatus::Open),
        "in_progress" => Ok(TaskStatus::InProgress),
        "done" => Ok(TaskStatus::Done),
        "cancelled" => Ok(TaskStatus::Cancelled),
        other => Err(EmberTroveError::Internal(format!(
            "unknown task_status: {other}"
        ))),
    }
}

fn parse_priority(s: &str) -> Result<TaskPriority, EmberTroveError> {
    match s {
        "low" => Ok(TaskPriority::Low),
        "medium" => Ok(TaskPriority::Medium),
        "high" => Ok(TaskPriority::High),
        other => Err(EmberTroveError::Internal(format!(
            "unknown task_priority: {other}"
        ))),
    }
}

pub fn parse_recurrence(s: &str) -> Result<RecurrenceRule, EmberTroveError> {
    match s {
        "daily" => Ok(RecurrenceRule::Daily),
        "weekly" => Ok(RecurrenceRule::Weekly),
        "biweekly" => Ok(RecurrenceRule::Biweekly),
        "monthly" => Ok(RecurrenceRule::Monthly),
        "yearly" => Ok(RecurrenceRule::Yearly),
        other => Err(EmberTroveError::Internal(format!(
            "unknown recurrence: {other}"
        ))),
    }
}

fn status_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Open => "open",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn priority_str(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::Low => "low",
        TaskPriority::Medium => "medium",
        TaskPriority::High => "high",
    }
}

pub fn recurrence_str(r: &RecurrenceRule) -> &'static str {
    match r {
        RecurrenceRule::Daily => "daily",
        RecurrenceRule::Weekly => "weekly",
        RecurrenceRule::Biweekly => "biweekly",
        RecurrenceRule::Monthly => "monthly",
        RecurrenceRule::Yearly => "yearly",
    }
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
        })
    }
}

const SELECT_COLS: &str = r#"
    id, node_id, owner_id, title,
    status::text    AS status,
    priority::text  AS priority,
    focus_date, due_date, recurrence, sort_order, created_at, updated_at
"#;

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
    node_title: Option<String>,
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
    }
    .into_task()?;
    Ok(MyDayTask {
        task,
        node_title: r.node_title,
    })
}

#[async_trait]
impl TaskRepo for PgTaskRepo {
    async fn create(
        &self,
        node_id: Option<NodeId>,
        owner_id: &str,
        req: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError> {
        // node_id from path param takes precedence; fall back to body field.
        let resolved_node_id = node_id.or(req.node_id).map(|n| n.0);
        let status = req.status.as_ref().map_or("open", |s| status_str(s));
        let priority = req.priority.as_ref().map_or("medium", |p| priority_str(p));
        let recurrence = req
            .recurrence
            .as_ref()
            .map(|r| recurrence_str(r).to_string());

        let row = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            INSERT INTO node_tasks
                (node_id, owner_id, title, status, priority, focus_date, due_date, recurrence)
            VALUES ($1, $2, $3, $4::task_status, $5::task_priority, $6, $7, $8)
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
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("create task failed: {e}")))?;

        row.into_task()
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
            r#"SELECT {SELECT_COLS} FROM node_tasks WHERE id = $1"#
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get task failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("task {id} not found")))?;

        row.into_task()
    }

    async fn update(&self, id: TaskId, req: UpdateTaskRequest) -> Result<Task, EmberTroveError> {
        let status_s = req.status.as_ref().map(|s| status_str(s).to_string());
        let priority_s = req.priority.as_ref().map(|p| priority_str(p).to_string());
        let recurrence_update = req.recurrence.is_some();
        let recurrence_val = req
            .recurrence
            .and_then(|r| r)
            .map(|r| recurrence_str(&r).to_string());
        let node_id_update = req.node_id.is_some();
        let node_id_val = req.node_id.and_then(|n| n).map(|n| n.0);

        let row = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            UPDATE node_tasks SET
                title      = COALESCE($2, title),
                status     = COALESCE($3::task_status,   status),
                priority   = COALESCE($4::task_priority, priority),
                focus_date = CASE WHEN $5  THEN $6  ELSE focus_date END,
                due_date   = CASE WHEN $7  THEN $8  ELSE due_date   END,
                recurrence = CASE WHEN $9  THEN $10 ELSE recurrence END,
                node_id    = CASE WHEN $11 THEN $12 ELSE node_id    END
            WHERE id = $1
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
        let result = sqlx::query("DELETE FROM node_tasks WHERE id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete task failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!("task {id} not found")));
        }
        Ok(())
    }

    async fn list_my_day(
        &self,
        owner_id: &str,
        date: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError> {
        let rows = sqlx::query_as::<_, MyDayRow>(
            r#"
            SELECT
                t.id, t.node_id, t.owner_id, t.title,
                t.status::text    AS status,
                t.priority::text  AS priority,
                t.focus_date, t.due_date, t.recurrence, t.sort_order,
                t.created_at, t.updated_at,
                n.title           AS node_title
            FROM node_tasks t
            LEFT JOIN nodes n ON n.id = t.node_id
            WHERE t.owner_id = $1
              AND t.focus_date IS NOT NULL
              AND t.focus_date <= $2
              AND (
                  t.focus_date = $2
                  OR t.status::text NOT IN ('done', 'cancelled')
              )
            ORDER BY
                t.sort_order ASC,
                CASE t.priority::text
                    WHEN 'high'   THEN 0
                    WHEN 'medium' THEN 1
                    WHEN 'low'    THEN 2
                END,
                t.node_id NULLS FIRST,
                t.created_at
            "#,
        )
        .bind(owner_id)
        .bind(date)
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
              AND node_id IS NULL
            ORDER BY
                sort_order ASC,
                CASE priority::text
                    WHEN 'high'   THEN 0
                    WHEN 'medium' THEN 1
                    WHEN 'low'    THEN 2
                END,
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
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError> {
        let rows = sqlx::query_as::<_, MyDayRow>(
            r#"
            SELECT
                t.id, t.node_id, t.owner_id, t.title,
                t.status::text    AS status,
                t.priority::text  AS priority,
                t.focus_date, t.due_date, t.recurrence, t.sort_order,
                t.created_at, t.updated_at,
                n.title           AS node_title
            FROM node_tasks t
            LEFT JOIN nodes n ON n.id = t.node_id
            WHERE t.owner_id = $1
              AND t.due_date >= $2
              AND t.due_date <= $3
            ORDER BY t.due_date,
                CASE t.priority::text
                    WHEN 'high'   THEN 0
                    WHEN 'medium' THEN 1
                    WHEN 'low'    THEN 2
                END,
                t.created_at
            "#,
        )
        .bind(owner_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list by due range failed: {e}")))?;

        rows.into_iter().map(my_day_row_to_task).collect()
    }

    async fn list_open_for_owner(&self, owner_id: &str) -> Result<Vec<MyDayTask>, EmberTroveError> {
        // NULLS LAST on due_date so dated tasks float to the top of the
        // backlog; undated open tasks settle at the bottom.  Priority desc
        // (high → low) breaks ties.  created_at ASC keeps order stable
        // across re-fetches for items with no due date.
        let rows = sqlx::query_as::<_, MyDayRow>(
            r#"
            SELECT
                t.id, t.node_id, t.owner_id, t.title,
                t.status::text    AS status,
                t.priority::text  AS priority,
                t.focus_date, t.due_date, t.recurrence, t.sort_order,
                t.created_at, t.updated_at,
                n.title           AS node_title
            FROM node_tasks t
            LEFT JOIN nodes n ON n.id = t.node_id
            WHERE t.owner_id = $1
              AND t.status::text NOT IN ('done', 'cancelled')
            ORDER BY
                t.due_date ASC NULLS LAST,
                CASE t.priority::text
                    WHEN 'high'   THEN 0
                    WHEN 'medium' THEN 1
                    WHEN 'low'    THEN 2
                END,
                t.created_at ASC
            "#,
        )
        .bind(owner_id)
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
            in_progress: i64,
            done: i64,
            cancelled: i64,
        }

        let rows = sqlx::query_as::<_, CountRow>(
            r#"
            SELECT
                node_id,
                COUNT(*) FILTER (WHERE status = 'open')        AS open,
                COUNT(*) FILTER (WHERE status = 'in_progress') AS in_progress,
                COUNT(*) FILTER (WHERE status = 'done')        AS done,
                COUNT(*) FILTER (WHERE status = 'cancelled')   AS cancelled
            FROM node_tasks
            WHERE node_id = ANY($1)
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
                        in_progress: r.in_progress as u32,
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
        let rows = sqlx::query_as::<_, SummaryRow>(
            r#"
            SELECT node_id, id, title, status::text AS status, priority::text AS priority,
                   due_date, rn
            FROM (
                SELECT node_id, id, title, status, priority, due_date,
                       ROW_NUMBER() OVER (
                           PARTITION BY node_id
                           ORDER BY
                               CASE priority::text
                                   WHEN 'high'   THEN 0
                                   WHEN 'medium' THEN 1
                                   WHEN 'low'    THEN 2
                               END,
                               due_date NULLS LAST,
                               sort_order ASC,
                               created_at ASC
                       ) AS rn
                FROM node_tasks
                WHERE node_id = ANY($1)
                  AND status::text IN ('open', 'in_progress')
            ) ranked
            WHERE rn <= $2
            ORDER BY node_id, rn
            "#,
        )
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
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            r#"
            SELECT {SELECT_COLS}
            FROM node_tasks
            ORDER BY created_at ASC
            "#
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all tasks failed: {e}")))?;

        rows.into_iter().map(TaskRow::into_task).collect()
    }

    async fn reorder(
        &self,
        updates: &[(TaskId, i32)],
        owner_id: &str,
    ) -> Result<(), EmberTroveError> {
        if updates.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = updates.iter().map(|(id, _)| id.0).collect();
        let orders: Vec<i32> = updates.iter().map(|(_, o)| *o).collect();

        sqlx::query(
            r#"
            UPDATE node_tasks
            SET sort_order = u.sort_order
            FROM (
                SELECT
                    unnest($1::uuid[])  AS id,
                    unnest($2::int4[])  AS sort_order
            ) AS u
            WHERE node_tasks.id = u.id
              AND node_tasks.owner_id = $3
            "#,
        )
        .bind(&ids)
        .bind(&orders)
        .bind(owner_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("reorder tasks failed: {e}")))?;

        Ok(())
    }
}
