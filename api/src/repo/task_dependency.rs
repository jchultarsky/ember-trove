//! Tasks that wait for other tasks (S5d; `common::task_dependency`,
//! migration 055).
//!
//! SECURITY: a link is shown and counted only to a reader who can see its
//! other task: that task is live (not in the trash) and on a node they
//! can view (ONTOLOGY §14). [`shown_to`] is the one spelling of that
//! rule, and [`blocked_by_count`] the one spelling of "how many
//! prerequisites hold this task back"; the lists here, the remove, and
//! the task feeds in `repo::task` all use them, so none can drift.
//!
//! Two things look at every link, hidden and trashed ones included: the
//! loop check, and through it the picker's candidates. They have to: a
//! loop is not a storable state, and a link through a trashed task comes
//! back when the task is restored. So someone who may edit one task and
//! view another can learn that a chain runs between them, never through
//! what. Node edges tell the same (`repo::edge::closes_loop`).
//!
//! A link nobody is shown would otherwise stay for good and still decide
//! those refusals: someone could plant one in another person's tasks, or
//! leave one behind when their share ends (security review, 2026-11-16).
//! So whoever may edit a task is told how many of its prerequisites they
//! aren't shown ([`TaskDependencyRepo::not_shown`]), never which, and may
//! remove them ([`TaskDependencyRepo::remove_not_shown`]). It is the rule
//! a home its reader can't see got (owner decision, 2026-10-31).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::{NodeId, TaskId},
    task::TaskStatus,
    task_dependency::{
        CANDIDATE_LIMIT, DependencyRefusal, MAX_PREREQUISITES, TaskDependency, TaskLink,
        holding_back_states,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

use super::node::{DEADLOCK, LOCK_NOT_AVAILABLE, sqlstate};
use super::{escape_like, visible_among_many, visible_to};

/// Postgres' SQLSTATE for a statement ended by `statement_timeout`.
const QUERY_CANCELED: &str = "57014";

/// `add` holds one lock for every account's dependencies while it checks
/// and writes. It must neither wait for it for long nor hold it for long:
/// a stuck or very slow one would queue every other add behind it, each
/// on a connection of its own.
const ADD_TIMEOUTS: [&str; 2] = [
    "SET LOCAL lock_timeout = '2s'",
    "SET LOCAL statement_timeout = '5s'",
];

/// What the API says for a task that isn't there, or that its caller
/// can't see: one sentence for both.
const TASK_NOT_FOUND: &str = "task not found";

#[async_trait]
pub trait TaskDependencyRepo: Send + Sync {
    /// The tasks `task` waits for, and the tasks that wait for it, as
    /// `viewer` sees them (`None`: an admin, who reads every node). The
    /// caller has checked that `viewer` may view `task`.
    async fn list(
        &self,
        task: TaskId,
        viewer: Option<&str>,
    ) -> Result<(Vec<TaskLink>, Vec<TaskLink>), EmberTroveError>;

    /// Make `task` wait for `depends_on`. The caller has checked the
    /// rights on both. Refuses a loop and a task with too many
    /// prerequisites (422), and a repeat (409); a task deleted meanwhile
    /// is NotFound. When the lock isn't had in time: 409, "try again".
    async fn add(&self, task: TaskId, depends_on: TaskId) -> Result<(), EmberTroveError>;

    /// How many of `task`'s prerequisites `viewer` is not shown: in the
    /// trash, or on a node they can't view. For someone who may edit
    /// `task` only: the caller has checked that.
    async fn not_shown(&self, task: TaskId, viewer: Option<&str>) -> Result<u32, EmberTroveError>;

    /// Remove every link from `task` to a prerequisite `viewer` is not
    /// shown, and say how many went. The caller has checked that `viewer`
    /// may edit `task`.
    async fn remove_not_shown(
        &self,
        task: TaskId,
        viewer: Option<&str>,
    ) -> Result<u64, EmberTroveError>;

    /// Stop `task` waiting for `depends_on`. Only a link `viewer` is
    /// shown is removed; any other, and no link at all, is the same
    /// NotFound.
    async fn remove(
        &self,
        task: TaskId,
        depends_on: TaskId,
        viewer: Option<&str>,
    ) -> Result<(), EmberTroveError>;

    /// What `task`'s prerequisites wait for in turn (S7b, §9.3), and
    /// whether there was more than was read. Only through tasks
    /// `viewer` is shown that still hold back, going on only through
    /// ones still to do; a hidden or trashed task ends the walk, and
    /// nothing beyond it is shown. A `task` that `viewer` is not shown
    /// has no chain, whoever asks: the route checks that too, and this
    /// doesn't rely on it.
    async fn chain(
        &self,
        task: TaskId,
        viewer: Option<&str>,
    ) -> Result<(Vec<common::task_dependency::ChainedTask>, bool), EmberTroveError>;

    /// For each of `node_ids` that has any: how many of its tasks still
    /// to do wait for a task that is not done (S7b): the rows of its
    /// task list that say "Blocked" for `viewer`
    /// ([`blocked_by_count`], the one spelling). A node `viewer` can't
    /// view gets no count, whoever asks.
    async fn blocked_tasks_for_nodes(
        &self,
        node_ids: &[NodeId],
        viewer: Option<&str>,
    ) -> Result<Vec<(NodeId, u32)>, EmberTroveError>;

    /// The tasks `subject` could make `task` wait for, matching `q`:
    /// still to do, on a node they own or were given, and not one that
    /// would be refused. `subject` is the person, never an admin's
    /// "everyone": the picker is not widened.
    async fn candidates(
        &self,
        task: TaskId,
        subject: &str,
        q: &str,
    ) -> Result<Vec<TaskLink>, EmberTroveError>;
}

/// SQL predicate: the task aliased `task`, on the node aliased `node`,
/// is one the reader bound at `$param` is shown: it is not in the trash,
/// and they can view its node.
///
/// SECURITY: the aliases and `param` come from the code, never from a
/// request; the reader is always a bind parameter.
pub(crate) fn shown_to(task: &str, node: &str, param: u8) -> String {
    format!(
        "({task}.deleted_at IS NULL AND {})",
        visible_to(node, param)
    )
}

/// SQL predicate: the task aliased `task` still holds its dependents
/// back. Built from `common::task_dependency::holds_back` through its
/// list of states, so the SQL can't name a different set. The words are
/// the enum's own wire strings, never text from a request.
pub(crate) fn holds_back_sql(task: &str) -> String {
    let states = holding_back_states()
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{task}.status::text IN ({states})")
}

/// SQL predicate: the prerequisite `p` (on node `pn`) of a task on node
/// `tn` is one its editors are told of by count and may clear: it is
/// shown **neither to the reader** (bound at `$param`) **nor to the
/// owner of the task's node** (in the trash, or on a node they can't
/// see).
///
/// SECURITY: the owner half (#349) keeps this off the owner's own
/// prerequisites. Without it, someone a Project is shared with as an
/// editor was told how many of a task's prerequisites sit on the
/// owner's private Projects, and could remove them all. The owner is a
/// column, in SQL: bound as an optional value, a NULL would read as
/// "sees everything".
///
/// A prerequisite in the trash counts only where the reader would see
/// it but for the trash (its node is visible to them), or where the
/// owner can't see its node either. One on a node the owner sees and
/// the reader doesn't stays the owner's business, in the trash or out
/// of it: an editor who cleared it would undo the owner's dependency
/// for the day the task is restored.
///
/// "The owner" is the account the task's node belongs to
/// (`nodes.owner_id`), seeing by ownership or by a grant. Someone else
/// who holds an Owner grant is not meant, and nor is what an admin sees
/// by role: SQL can't read a token's roles.
fn not_shown_sql(param: u8) -> String {
    format!(
        "(NOT {reader_shown} AND (NOT {owner_sees} \
          OR (p.deleted_at IS NOT NULL AND {reader_sees_node})))",
        reader_shown = shown_to("p", "pn", param),
        reader_sees_node = visible_to("pn", param),
        owner_sees = super::owned_or_granted("pn", "tn.owner_id"),
    )
}

/// SQL predicate: a chain goes on through the task aliased `task`
/// (`common::task_dependency::passes_on`): it is still to do. From the
/// enum, like [`holds_back_sql`].
pub(crate) fn passes_on_sql(task: &str) -> String {
    let states = TaskStatus::ALL
        .iter()
        .filter(|s| common::task_dependency::passes_on(s))
        .map(|s| format!("'{}'", s.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{task}.status::text IN ({states})")
}

/// SQL expression: how many prerequisites of the task whose id is the
/// expression `task_id` hold it back, for the reader bound at `$param`.
/// A correlated count with its own visibility test: a prerequisite sits
/// on its own node, which is not the node the outer query joined.
pub(crate) fn blocked_by_count(task_id: &str, param: u8) -> String {
    format!(
        "(SELECT count(*)::int4 FROM task_dependencies bd \
          JOIN node_tasks bp ON bp.id = bd.depends_on_id \
          JOIN nodes bn ON bn.id = bp.node_id \
          WHERE bd.task_id = {task_id} AND {shown} AND {holds})",
        shown = shown_to("bp", "bn", param),
        holds = holds_back_sql("bp"),
    )
}

/// A count read as `int4` (never negative) as the `u32` the DTOs carry.
pub(crate) fn as_count(n: i32) -> u32 {
    u32::try_from(n).unwrap_or(0)
}

/// Every link between two live tasks. A free function so the backup can
/// run it on its one snapshot transaction (`backup::read_snapshot`): the
/// archive holds live tasks only, so a link to a trashed one would name
/// a task it doesn't carry.
pub(crate) async fn list_all_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<TaskDependency>, EmberTroveError> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, DateTime<Utc>)>(
        r#"
        SELECT d.task_id, d.depends_on_id, d.created_at
        FROM task_dependencies d
        JOIN node_tasks a ON a.id = d.task_id AND a.deleted_at IS NULL
        JOIN node_tasks b ON b.id = d.depends_on_id AND b.deleted_at IS NULL
        ORDER BY d.created_at, d.task_id, d.depends_on_id
        "#,
    )
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list task dependencies failed: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|(task, depends_on, created_at)| TaskDependency {
            task_id: TaskId(task),
            depends_on_id: TaskId(depends_on),
            created_at,
        })
        .collect())
}

pub struct PgTaskDependencyRepo {
    pool: PgPool,
}

impl PgTaskDependencyRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: Uuid,
    title: String,
    status: String,
    node_id: Uuid,
    node_title: String,
}

impl LinkRow {
    fn into_link(self) -> Result<TaskLink, EmberTroveError> {
        Ok(TaskLink {
            id: TaskId(self.id),
            title: self.title,
            status: super::parse_wire(&self.status, "task_status", TaskStatus::parse)?,
            node_id: NodeId(self.node_id),
            node_title: self.node_title,
        })
    }
}

/// The columns of a [`LinkRow`]: the other task `p` and its node `pn`.
const LINK_COLS: &str =
    "p.id, p.title, p.status::text AS status, p.node_id, pn.title AS node_title";

fn links(rows: Vec<LinkRow>) -> Result<Vec<TaskLink>, EmberTroveError> {
    rows.into_iter().map(LinkRow::into_link).collect()
}

/// Would `task` waiting for `depends_on` close a loop: does `depends_on`
/// already wait for `task`, directly or through others? Over every link,
/// whoever can see it. The caller holds the lock.
async fn closes_loop(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task: TaskId,
    depends_on: TaskId,
) -> Result<bool, sqlx::Error> {
    // UNION (not UNION ALL), as the edges' check: a loop that got in by
    // another way must not recurse forever.
    sqlx::query_scalar(
        r#"
        WITH RECURSIVE reach(id) AS (
            SELECT depends_on_id FROM task_dependencies WHERE task_id = $2
            UNION
            SELECT d.depends_on_id FROM task_dependencies d JOIN reach r ON d.task_id = r.id
        )
        SELECT EXISTS (SELECT 1 FROM reach WHERE id = $1)
        "#,
    )
    .bind(task.0)
    .bind(depends_on.0)
    .fetch_one(&mut **tx)
    .await
}

#[async_trait]
impl TaskDependencyRepo for PgTaskDependencyRepo {
    async fn list(
        &self,
        task: TaskId,
        viewer: Option<&str>,
    ) -> Result<(Vec<TaskLink>, Vec<TaskLink>), EmberTroveError> {
        let failed =
            |e: sqlx::Error| EmberTroveError::Internal(format!("list dependencies failed: {e}"));
        let shown = shown_to("p", "pn", 2);
        let blocked_by = sqlx::query_as::<_, LinkRow>(&format!(
            r#"
            SELECT {LINK_COLS}
            FROM task_dependencies d
            JOIN node_tasks p ON p.id = d.depends_on_id
            JOIN nodes pn ON pn.id = p.node_id
            WHERE d.task_id = $1 AND {shown}
            ORDER BY d.created_at, p.id
            "#
        ))
        .bind(task.0)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await
        .map_err(failed)?;
        let blocks = sqlx::query_as::<_, LinkRow>(&format!(
            r#"
            SELECT {LINK_COLS}
            FROM task_dependencies d
            JOIN node_tasks p ON p.id = d.task_id
            JOIN nodes pn ON pn.id = p.node_id
            WHERE d.depends_on_id = $1 AND {shown}
            ORDER BY d.created_at, p.id
            "#
        ))
        .bind(task.0)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await
        .map_err(failed)?;
        Ok((links(blocked_by)?, links(blocks)?))
    }

    async fn chain(
        &self,
        task: TaskId,
        viewer: Option<&str>,
    ) -> Result<(Vec<common::task_dependency::ChainedTask>, bool), EmberTroveError> {
        use common::blockers::CHAIN_LIMIT;
        use common::task_dependency::{TaskHop, chained};
        #[derive(sqlx::FromRow)]
        struct Row {
            /// The link's place in the order the walk found it.
            n: i64,
            from_id: Uuid,
            #[sqlx(flatten)]
            link: LinkRow,
        }
        // As `EdgeRepo::blockers`: the row is the link alone and the
        // halves are joined with UNION, so a loop ends. A link is taken
        // when the task it leads to is shown to the reader and still
        // holds back; the walk starts from a task still to do and goes
        // on only through such a task.
        let rows = sqlx::query_as::<_, Row>(&format!(
            r#"
            WITH RECURSIVE walk(task_id, depends_on_id) AS (
                SELECT d.task_id, d.depends_on_id
                FROM task_dependencies d
                JOIN node_tasks s ON s.id = d.task_id
                JOIN nodes sn ON sn.id = s.node_id
                JOIN node_tasks p ON p.id = d.depends_on_id
                JOIN nodes pn ON pn.id = p.node_id
                WHERE d.task_id = $1
                  AND {s_shown} AND {s_goes_on}
                  AND {shown} AND {holds}
              UNION
                SELECT d.task_id, d.depends_on_id
                FROM walk w
                JOIN node_tasks via ON via.id = w.depends_on_id
                JOIN task_dependencies d ON d.task_id = w.depends_on_id
                JOIN node_tasks p ON p.id = d.depends_on_id
                JOIN nodes pn ON pn.id = p.node_id
                WHERE {via_goes_on}
                  AND {shown} AND {holds}
            ),
            cut AS (
                SELECT row_number() OVER () AS n, task_id, depends_on_id
                FROM walk
                LIMIT $3
            )
            SELECT c.n, c.task_id AS from_id, {LINK_COLS}
            FROM cut c
            JOIN node_tasks p ON p.id = c.depends_on_id
            JOIN nodes pn ON pn.id = p.node_id
            "#,
            s_shown = shown_to("s", "sn", 2),
            s_goes_on = passes_on_sql("s"),
            via_goes_on = passes_on_sql("via"),
            shown = shown_to("p", "pn", 2),
            holds = holds_back_sql("p"),
        ))
        .bind(task.0)
        .bind(viewer)
        .bind(i64::try_from(CHAIN_LIMIT + 1).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("read task chain failed: {e}")))?;

        // The link past the limit is the last one found: it says there
        // is more and is left out. Trimming by the join's order, which
        // may differ, could drop a link from the middle of the chain.
        let limit = i64::try_from(CHAIN_LIMIT).unwrap_or(i64::MAX);
        let truncated = rows.iter().any(|r| r.n > limit);
        let hops = rows
            .into_iter()
            .filter(|r| r.n <= limit)
            .map(|r| {
                Ok(TaskHop {
                    from: TaskId(r.from_id),
                    to: r.link.into_link()?,
                })
            })
            .collect::<Result<Vec<_>, EmberTroveError>>()?;
        Ok((chained(task, hops), truncated))
    }

    async fn blocked_tasks_for_nodes(
        &self,
        node_ids: &[NodeId],
        viewer: Option<&str>,
    ) -> Result<Vec<(NodeId, u32)>, EmberTroveError> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = node_ids.iter().map(|n| n.0).collect();
        let rows: Vec<(Uuid, i32)> = sqlx::query_as(&format!(
            r#"
            SELECT t.node_id, count(*)::int4
            FROM node_tasks t
            JOIN nodes tn ON tn.id = t.node_id
            WHERE t.node_id = ANY($1)
              AND {t_shown}
              AND {goes_on}
              AND {blocked_by} > 0
            GROUP BY t.node_id
            "#,
            t_shown = shown_to("t", "tn", 2),
            // Still to do: a finished task is never blocked
            // (`task_dependency::is_blocked`).
            goes_on = passes_on_sql("t"),
            blocked_by = blocked_by_count("t.id", 2),
        ))
        .bind(&ids)
        .bind(viewer)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("blocked tasks failed: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(id, n)| (NodeId(id), as_count(n)))
            .collect())
    }

    async fn add(&self, task: TaskId, depends_on: TaskId) -> Result<(), EmberTroveError> {
        // The lock not had in time, a statement that ran too long, or a
        // deadlock: nothing was written. 409, "try again", as the cascade
        // answers.
        let failed = |e: sqlx::Error| match sqlstate(&e) {
            Some(LOCK_NOT_AVAILABLE | QUERY_CANCELED | DEADLOCK) => EmberTroveError::AlreadyExists(
                "another change to task dependencies is in progress: try again".to_string(),
            ),
            _ => EmberTroveError::Internal(format!("add dependency failed: {e}")),
        };
        let mut tx = self.pool.begin().await.map_err(failed)?;
        for timeout in ADD_TIMEOUTS {
            sqlx::query(timeout)
                .execute(&mut *tx)
                .await
                .map_err(failed)?;
        }

        // One lock for all task dependencies: without it two inserts
        // could each pass the loop check below and together close one
        // (the edges' rule, #224).
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ember.task_dependencies'))")
            .execute(&mut *tx)
            .await
            .map_err(failed)?;

        // Every link of the task counts, whoever is shown it: the limit
        // bounds what is stored. (A count of the shown ones alone could
        // be stepped round through the trash.)
        let held: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_dependencies WHERE task_id = $1")
                .bind(task.0)
                .fetch_one(&mut *tx)
                .await
                .map_err(failed)?;
        if usize::try_from(held).unwrap_or(usize::MAX) >= MAX_PREREQUISITES {
            return Err(EmberTroveError::Validation(
                DependencyRefusal::TooMany.message(),
            ));
        }

        if closes_loop(&mut tx, task, depends_on)
            .await
            .map_err(failed)?
        {
            return Err(EmberTroveError::Validation(
                DependencyRefusal::Loop.message(),
            ));
        }

        sqlx::query("INSERT INTO task_dependencies (task_id, depends_on_id) VALUES ($1, $2)")
            .bind(task.0)
            .bind(depends_on.0)
            .execute(&mut *tx)
            .await
            .map_err(|e| match &e {
                sqlx::Error::Database(d)
                    if d.constraint() == Some("task_dependencies_not_itself") =>
                {
                    EmberTroveError::Validation(DependencyRefusal::Itself.message())
                }
                sqlx::Error::Database(d) if d.is_unique_violation() => {
                    EmberTroveError::AlreadyExists(
                        "this task already waits for that one".to_string(),
                    )
                }
                // Either task was deleted for good since the route read it
                // (the trash emptied, its node deleted, its account purged).
                sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
                    EmberTroveError::NotFound(TASK_NOT_FOUND.to_string())
                }
                _ => failed(e),
            })?;
        tx.commit().await.map_err(failed)
    }

    async fn remove(
        &self,
        task: TaskId,
        depends_on: TaskId,
        viewer: Option<&str>,
    ) -> Result<(), EmberTroveError> {
        let result = sqlx::query(&format!(
            r#"
            DELETE FROM task_dependencies d
            USING node_tasks p, nodes pn
            WHERE d.task_id = $1 AND d.depends_on_id = $2
              AND p.id = d.depends_on_id AND pn.id = p.node_id
              AND {shown}
            "#,
            shown = shown_to("p", "pn", 3),
        ))
        .bind(task.0)
        .bind(depends_on.0)
        .bind(viewer)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("remove dependency failed: {e}")))?;
        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(
                "dependency not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn not_shown(&self, task: TaskId, viewer: Option<&str>) -> Result<u32, EmberTroveError> {
        let n: i32 = sqlx::query_scalar(&format!(
            r#"
            SELECT count(*)::int4
            FROM task_dependencies d
            JOIN node_tasks t ON t.id = d.task_id
            JOIN nodes tn ON tn.id = t.node_id
            JOIN node_tasks p ON p.id = d.depends_on_id
            JOIN nodes pn ON pn.id = p.node_id
            WHERE d.task_id = $1 AND {hidden}
            "#,
            hidden = not_shown_sql(2),
        ))
        .bind(task.0)
        .bind(viewer)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("count dependencies failed: {e}")))?;
        Ok(as_count(n))
    }

    async fn remove_not_shown(
        &self,
        task: TaskId,
        viewer: Option<&str>,
    ) -> Result<u64, EmberTroveError> {
        let result = sqlx::query(&format!(
            r#"
            DELETE FROM task_dependencies d
            USING node_tasks t, nodes tn, node_tasks p, nodes pn
            WHERE d.task_id = $1
              AND t.id = d.task_id AND tn.id = t.node_id
              AND p.id = d.depends_on_id AND pn.id = p.node_id
              AND {hidden}
            "#,
            hidden = not_shown_sql(2),
        ))
        .bind(task.0)
        .bind(viewer)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("remove dependencies failed: {e}")))?;
        Ok(result.rows_affected())
    }

    async fn candidates(
        &self,
        task: TaskId,
        subject: &str,
        q: &str,
    ) -> Result<Vec<TaskLink>, EmberTroveError> {
        let still_to_do = super::task::still_to_do()
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(", ");
        // `waits`: every task that already waits for this one, directly
        // or through others. Making this task wait for one of them would
        // close a loop, which `add` refuses ([`closes_loop`] walks the
        // same links the other way), so the picker never offers it.
        let rows = sqlx::query_as::<_, LinkRow>(&format!(
            r#"
            WITH RECURSIVE waits(id) AS (
                SELECT task_id FROM task_dependencies WHERE depends_on_id = $1
                UNION
                SELECT d.task_id FROM task_dependencies d JOIN waits w ON d.depends_on_id = w.id
            )
            SELECT {LINK_COLS}
            FROM node_tasks p
            JOIN nodes pn ON pn.id = p.node_id
            WHERE p.id <> $1
              AND p.deleted_at IS NULL
              AND {visible}
              AND p.status::text IN ({still_to_do})
              AND pn.status <> 'archived'::node_status
              AND NOT EXISTS (
                  SELECT 1 FROM task_dependencies x
                  WHERE x.task_id = $1 AND x.depends_on_id = p.id)
              AND p.id NOT IN (SELECT id FROM waits)
              AND ($3 = '' OR p.title ILIKE '%' || $3 || '%' ESCAPE '\')
            ORDER BY
                (p.node_id = (SELECT node_id FROM node_tasks WHERE id = $1)) DESC,
                lower(p.title) COLLATE "C",
                p.id
            LIMIT $4
            "#,
            // `shown_to`'s rule in the spelling for a scan over many of
            // the person's nodes (`visible_among_many`): this reads every
            // task still to do, not the few links of one task.
            visible = visible_among_many("pn", 2),
        ))
        .bind(task.0)
        .bind(subject)
        .bind(escape_like(q.trim()))
        .bind(CANDIDATE_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("dependency candidates failed: {e}")))?;
        links(rows)
    }
}
