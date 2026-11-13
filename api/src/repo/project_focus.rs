//! Focus (#282, proposal B): the projects that hold a person's attention
//! (`common::project_focus`).
//!
//! SECURITY: Focus is private to its person, with no admin bypass (owner
//! decision, 2026-11-09), like captures and people. Every method takes the
//! person as `&str`, never `Option`, and every statement matches
//! `subject_id`, so another account (an admin included) reads and changes
//! nothing of it. Only `list_all`, for the admin-run backup, reads across
//! accounts.
//!
//! The reads also check, in the same statement, that the reader can still
//! see the project (`visible_to`) and that it still holds Focus
//! (`common::project_focus::holds_focus`). So a project whose share was
//! taken back drops out, and its title is never read. That is a filter:
//! the row stays, and nothing here deletes on a read.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    backup::BackupFocus,
    id::NodeId,
    node::Node,
    project_focus::{FocusItem, WhyChange},
};
use sqlx::PgPool;
use uuid::Uuid;

use super::node::NodeRow;

/// A Focus entry and its project, read together under one visibility
/// check.
#[derive(Debug, Clone)]
pub struct FocusedProject {
    pub item: FocusItem,
    pub node: Node,
}

#[async_trait]
pub trait FocusRepo: Send + Sync {
    /// `subject`'s Focus, in their order. `sees_all` is whether they may
    /// read every node (an admin): it widens which of **their own**
    /// entries show, never whose entries.
    async fn list(
        &self,
        subject: &str,
        sees_all: bool,
    ) -> Result<Vec<FocusedProject>, EmberTroveError>;
    /// `subject`'s entry for `node`, under the same filter as [`list`]:
    /// `None` when there is none, or it doesn't show.
    ///
    /// [`list`]: FocusRepo::list
    async fn get(
        &self,
        subject: &str,
        sees_all: bool,
        node: NodeId,
    ) -> Result<Option<FocusItem>, EmberTroveError>;
    /// Put `node` in `subject`'s Focus, last. One already there keeps its
    /// place and its `added_at`. `why` is what happens to the why-line:
    /// kept as it is, or replaced. The caller has checked the rights and
    /// the kind.
    async fn put(
        &self,
        subject: &str,
        node: NodeId,
        why: &WhyChange,
    ) -> Result<FocusItem, EmberTroveError>;
    /// Replace the why-line of an entry that is there (`None` clears it).
    /// `NotFound` when `subject` has no entry for `node`: unlike [`put`],
    /// this never creates one, so a why-line saved from a stale page can't
    /// put a removed project back in Focus.
    ///
    /// [`put`]: FocusRepo::put
    async fn set_why(
        &self,
        subject: &str,
        node: NodeId,
        why_now: Option<&str>,
    ) -> Result<FocusItem, EmberTroveError>;
    /// Order `subject`'s entries as `nodes` lists them. An id that is not
    /// in their Focus is skipped, and so is a repeat of one. An entry
    /// `nodes` leaves out (one the list doesn't show: an archived project,
    /// a share taken back) keeps its place among the others.
    async fn reorder(&self, subject: &str, nodes: &[NodeId]) -> Result<(), EmberTroveError>;
    /// Take `node` out of `subject`'s Focus. No entry is not an error.
    async fn remove(&self, subject: &str, node: NodeId) -> Result<(), EmberTroveError>;
    /// Every account's entries: the admin-run backup only.
    async fn list_all(&self) -> Result<Vec<BackupFocus>, EmberTroveError>;
}

pub struct PgFocusRepo {
    pool: PgPool,
}

impl PgFocusRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    node_id: Uuid,
    why_now: Option<String>,
    added_at: DateTime<Utc>,
}

impl From<ItemRow> for FocusItem {
    fn from(r: ItemRow) -> Self {
        FocusItem {
            node_id: NodeId(r.node_id),
            why_now: r.why_now,
            added_at: r.added_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ListedRow {
    #[sqlx(flatten)]
    node: NodeRow,
    why_now: Option<String>,
    added_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct BackupRow {
    subject_id: String,
    node_id: Uuid,
    position: i32,
    why_now: Option<String>,
    added_at: DateTime<Utc>,
}

fn db(what: &'static str) -> impl Fn(sqlx::Error) -> EmberTroveError {
    move |e| EmberTroveError::Internal(format!("focus {what} failed: {e}"))
}

/// Which of a person's entries show: `f` is the entry, `n` its node, `$1`
/// the person and `$2` the subject whose view applies (NULL: every node).
/// The kind, lifecycle and state half is
/// `common::project_focus::holds_focus`: a Project, not archived, whose
/// declared state is active (S6a: a parked, complete or cancelled project
/// doesn't show). `the_focus_filter_is_the_shared_rule` holds the two
/// together.
fn shows() -> String {
    format!(
        "f.subject_id = $1 AND n.node_type = 'project' AND n.status <> 'archived' \
         AND n.project_state = 'active' AND {visible}",
        visible = super::visible_to("n", 2),
    )
}

/// Remove `subject`'s own entry for `node`, on the caller's transaction:
/// the write that closes a project (`NodeRepo::set_project_state`) takes
/// its declaring person's entry with it, so re-opening doesn't put the
/// project back in their Focus (ONTOLOGY §10). It lives here so every
/// statement that touches Focus is in this module. It matches
/// `subject_id` like all the others: nobody else's entry is ever removed,
/// and the caller must record nothing about it (History, logs and
/// webhooks are shared with the node).
pub(crate) async fn remove_on(
    conn: &mut sqlx::PgConnection,
    subject: &str,
    node: NodeId,
) -> Result<(), EmberTroveError> {
    sqlx::query("DELETE FROM project_focus WHERE subject_id = $1 AND node_id = $2")
        .bind(subject)
        .bind(node.0)
        .execute(conn)
        .await
        .map_err(db("remove on close"))?;
    Ok(())
}

/// The subject bound at `$2` of [`shows`].
fn view(subject: &str, sees_all: bool) -> Option<&str> {
    (!sees_all).then_some(subject)
}

/// Every entry, all accounts. A free function so the backup can run it on
/// its one snapshot transaction (`backup::read_snapshot`).
pub(crate) async fn list_all_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<BackupFocus>, EmberTroveError> {
    let rows: Vec<BackupRow> = sqlx::query_as(
        "SELECT subject_id, node_id, position, why_now, added_at FROM project_focus \
         ORDER BY subject_id, position, added_at, node_id",
    )
    .fetch_all(ex)
    .await
    .map_err(db("list all"))?;
    Ok(rows
        .into_iter()
        .map(|r| BackupFocus {
            subject_id: r.subject_id,
            node_id: NodeId(r.node_id),
            position: r.position,
            why_now: r.why_now,
            added_at: r.added_at,
        })
        .collect())
}

#[async_trait]
impl FocusRepo for PgFocusRepo {
    async fn list(
        &self,
        subject: &str,
        sees_all: bool,
    ) -> Result<Vec<FocusedProject>, EmberTroveError> {
        let rows: Vec<ListedRow> = sqlx::query_as(&format!(
            "SELECT {cols}, f.why_now, f.added_at \
             FROM project_focus f JOIN nodes n ON n.id = f.node_id \
             WHERE {shows} \
             ORDER BY f.position, f.added_at, f.node_id \
             LIMIT {limit}",
            cols = super::node::node_cols("n."),
            shows = shows(),
            limit = common::project_focus::MAX_ORDER,
        ))
        .bind(subject)
        .bind(view(subject, sees_all))
        .fetch_all(&self.pool)
        .await
        .map_err(db("list"))?;
        rows.into_iter()
            .map(|r| {
                let node = r.node.into_node()?;
                Ok(FocusedProject {
                    item: FocusItem {
                        node_id: node.id,
                        why_now: r.why_now,
                        added_at: r.added_at,
                    },
                    node,
                })
            })
            .collect()
    }

    async fn get(
        &self,
        subject: &str,
        sees_all: bool,
        node: NodeId,
    ) -> Result<Option<FocusItem>, EmberTroveError> {
        let row: Option<ItemRow> = sqlx::query_as(&format!(
            "SELECT f.node_id, f.why_now, f.added_at \
             FROM project_focus f JOIN nodes n ON n.id = f.node_id \
             WHERE {shows} AND f.node_id = $3",
            shows = shows(),
        ))
        .bind(subject)
        .bind(view(subject, sees_all))
        .bind(node.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(db("get"))?;
        Ok(row.map(FocusItem::from))
    }

    async fn put(
        &self,
        subject: &str,
        node: NodeId,
        why: &WhyChange,
    ) -> Result<FocusItem, EmberTroveError> {
        let (set, why_now) = match why {
            WhyChange::Keep => (false, None),
            WhyChange::Set(text) => (true, text.as_deref()),
        };
        // One statement: the aggregate always yields one row, so a first
        // entry gets position 0 and a later one goes last. Two adds at once
        // may share a position; the read breaks the tie by `added_at`.
        // On a conflict the row is always updated (to itself when the
        // why-line is kept), so RETURNING has it either way.
        let row: ItemRow = sqlx::query_as(
            "INSERT INTO project_focus (subject_id, node_id, position, why_now) \
             SELECT $1, $2, COALESCE(MAX(position), -1) + 1, $3 \
             FROM project_focus WHERE subject_id = $1 \
             ON CONFLICT (subject_id, node_id) DO UPDATE \
                 SET why_now = CASE WHEN $4 THEN EXCLUDED.why_now ELSE project_focus.why_now END \
             RETURNING node_id, why_now, added_at",
        )
        .bind(subject)
        .bind(node.0)
        .bind(why_now)
        .bind(set)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            // The node went between the route's check and this insert.
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
                EmberTroveError::NotFound("project not found".to_string())
            }
            _ => db("put")(e),
        })?;
        Ok(row.into())
    }

    async fn set_why(
        &self,
        subject: &str,
        node: NodeId,
        why_now: Option<&str>,
    ) -> Result<FocusItem, EmberTroveError> {
        let row: Option<ItemRow> = sqlx::query_as(
            "UPDATE project_focus SET why_now = $3 \
             WHERE subject_id = $1 AND node_id = $2 \
             RETURNING node_id, why_now, added_at",
        )
        .bind(subject)
        .bind(node.0)
        .bind(why_now)
        .fetch_optional(&self.pool)
        .await
        .map_err(db("set why"))?;
        row.map(FocusItem::from)
            .ok_or_else(|| EmberTroveError::NotFound("project not found".to_string()))
    }

    async fn reorder(&self, subject: &str, nodes: &[NodeId]) -> Result<(), EmberTroveError> {
        if nodes.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uuid> = nodes.iter().map(|n| n.0).collect();
        // One statement renumbers all of the person's entries, 0 upwards:
        // - `cur`: every entry in its present order (`slot`), and where
        //   the request wants it (`ord`; NULL when it isn't named, and the
        //   first mention counts when it is named twice).
        // - The named entries swap among **the slots they already hold**,
        //   in the wanted order. An entry the request leaves out keeps its
        //   slot. Numbering only the named ones from 0 would drop them on
        //   top of those, and a hidden project would come back somewhere
        //   else in the order.
        sqlx::query(
            "WITH cur AS ( \
                 SELECT f.node_id, w.ord, \
                        row_number() OVER (ORDER BY f.position, f.added_at, f.node_id) AS slot \
                 FROM project_focus f \
                 LEFT JOIN ( \
                     SELECT node_id, MIN(ord) AS ord \
                     FROM unnest($2::uuid[]) WITH ORDINALITY AS o(node_id, ord) \
                     GROUP BY node_id \
                 ) w ON w.node_id = f.node_id \
                 WHERE f.subject_id = $1 \
             ), \
             held AS ( \
                 SELECT slot, row_number() OVER (ORDER BY slot) AS k \
                 FROM cur WHERE ord IS NOT NULL \
             ), \
             wanted AS ( \
                 SELECT node_id, row_number() OVER (ORDER BY ord) AS k \
                 FROM cur WHERE ord IS NOT NULL \
             ), \
             next AS ( \
                 SELECT wanted.node_id, held.slot FROM wanted JOIN held USING (k) \
                 UNION ALL \
                 SELECT node_id, slot FROM cur WHERE ord IS NULL \
             ) \
             UPDATE project_focus f SET position = (next.slot - 1)::int \
             FROM next \
             WHERE f.subject_id = $1 AND f.node_id = next.node_id",
        )
        .bind(subject)
        .bind(&ids)
        .execute(&self.pool)
        .await
        .map_err(db("reorder"))?;
        Ok(())
    }

    async fn remove(&self, subject: &str, node: NodeId) -> Result<(), EmberTroveError> {
        sqlx::query("DELETE FROM project_focus WHERE subject_id = $1 AND node_id = $2")
            .bind(subject)
            .bind(node.0)
            .execute(&self.pool)
            .await
            .map_err(db("remove"))?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<BackupFocus>, EmberTroveError> {
        list_all_on(&self.pool).await
    }
}
