use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    activity::ActivityAction,
    cascade::CascadePlan,
    id::{NodeId, TagId},
    node::{
        CreateNodeRequest, Node, NodeDeletionPreview, NodeListParams, NodeSort, NodeStatus,
        NodeTitleEntry, NodeType, UpdateNodeRequest,
    },
    ontology::{Capability, allows},
    project_state::ProjectState,
    slug::slugify,
    tag::Tag,
};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::activity::{ActivityActor, record_in};
use uuid::Uuid;

#[async_trait]
pub trait NodeRepo: Send + Sync {
    async fn create(&self, owner_id: &str, req: CreateNodeRequest)
    -> Result<Node, EmberTroveError>;

    async fn get(&self, id: NodeId) -> Result<Node, EmberTroveError>;

    async fn get_by_slug(&self, slug: &str) -> Result<Node, EmberTroveError>;

    async fn list(&self, params: NodeListParams) -> Result<(Vec<Node>, u64), EmberTroveError>;

    /// Applies `req`. When it changes the project status, a `status_changed`
    /// History entry attributed to `actor` is written in the same
    /// transaction: the change and its record commit or fail together
    /// (#208, ONTOLOGY §10).
    async fn update(
        &self,
        id: NodeId,
        req: UpdateNodeRequest,
        actor: &ActivityActor,
    ) -> Result<Node, EmberTroveError>;

    async fn delete(&self, id: NodeId) -> Result<(), EmberTroveError>;

    /// What deleting `ids` would destroy with them (#281): live tasks,
    /// live notes and files, and how many of the tasks and notes belong to
    /// an account other than `caller`. Counts only. The route passes only
    /// nodes the caller may delete.
    async fn deletion_preview(
        &self,
        ids: &[NodeId],
        caller: &str,
    ) -> Result<NodeDeletionPreview, EmberTroveError>;

    async fn neighbors(
        &self,
        id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<Node>, EmberTroveError>;

    async fn backlinks(
        &self,
        id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<Node>, EmberTroveError>;

    /// Return nodes as lightweight title entries, ordered by title.
    /// Used for wiki-link autocomplete and resolution.
    ///
    /// - `subject = None` returns every node in the system (admin bypass).
    /// - `subject = Some(sub)` returns only nodes owned by `sub` or
    ///   explicitly shared with `sub` via the `permissions` table.
    async fn list_titles(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<NodeTitleEntry>, EmberTroveError>;

    /// Node-picker lookup: nodes visible to `subject` (admin: `None`, every
    /// node) whose title contains `q`, case-insensitively and literally
    /// (LIKE wildcards escaped). Ranked exact title, then prefix, then any
    /// other match, alphabetical within each; at most `limit` rows.
    async fn lookup_titles(
        &self,
        subject: Option<&str>,
        q: &str,
        limit: i64,
        node_type: Option<&NodeType>,
    ) -> Result<Vec<common::node::NodeLookupHit>, EmberTroveError>;

    /// Resolve many already-lowercased wiki-link titles in one query, among
    /// nodes **visible to `owner_id`** (the source node's owner): their own
    /// nodes, plus nodes shared with them. Their own node wins a title
    /// collision. Returns a map from the lowercased title to its node ID;
    /// unmatched titles are absent.
    ///
    /// SECURITY: resolution used to search every owner's titles, so writing
    /// `[[Someone Else's Title]]` created an edge into another user's private
    /// node and `neighbors` then returned that node's body. Links into nodes
    /// deliberately shared with the owner keep working.
    async fn find_ids_by_titles(
        &self,
        owner_id: &str,
        lowered_titles: &[String],
    ) -> Result<HashMap<String, NodeId>, EmberTroveError>;

    /// Fetch every node owned by `owner_id` with tags populated — used for backup.
    async fn list_all_for_owner(&self, owner_id: &str) -> Result<Vec<Node>, EmberTroveError>;

    /// Fetch every node across all owners with tags populated — used for full backup.
    async fn list_all(&self) -> Result<Vec<Node>, EmberTroveError>;

    /// For each `node_id` in the input, find its parent Area (via an
    /// inbound `edge_type = 'contains'` edge from a node whose
    /// `node_type = 'area'`).  Used by the v2.9.0 dashboard PARA grouping.
    ///
    /// Each project gets at most one Area parent; if multiple Areas
    /// `contain` the same project, the oldest edge (by `created_at`)
    /// wins.  Projects with no Area parent are absent from the result —
    /// the caller treats absence as "Ungrouped".
    ///
    /// Only Areas `subject` may read count (§14, #228): someone who can
    /// edit my Project may file it in their private Area, and its title
    /// must not reach my dashboard. `None` is the admin scope.
    async fn area_for_nodes(
        &self,
        node_ids: &[NodeId],
        subject: Option<&str>,
    ) -> Result<Vec<(NodeId, NodeId, String)>, EmberTroveError>;

    /// Toggle the `pinned` flag on a single node.  Used by the
    /// dashboard pin/unpin button (v2.9.0).
    async fn set_pinned(&self, id: NodeId, pinned: bool) -> Result<Node, EmberTroveError>;

    /// Declare a Project's state (S6a, S6b; ONTOLOGY §4.1). One
    /// transaction, holding the `contains` lock so the tree holds still:
    /// - the state is set, and `project_state_at` stamped when the state
    ///   changes or nobody had declared it yet (so declaring the state a
    ///   project already has records that it was looked at, once, and a
    ///   request sent twice moves nothing);
    /// - a change writes a `project_state_changed` History entry
    ///   attributed to `actor`;
    /// - a terminal state removes `actor`'s **own** Focus entry, so
    ///   re-opening doesn't put the project back there (§10). Nobody
    ///   else's entry is touched, and nothing records the removal.
    ///
    /// **The cascade** (S6b). A terminal state closes what is under the
    /// project: `common::cascade::plan`, recomputed here under the locks.
    /// - A *change* to a terminal state whose plan closes something needs
    ///   `confirm`, the token of the plan its sender was shown
    ///   ([`NodeRepo::cascade_preview`]). Without it, or with the token
    ///   of a plan that no longer holds, nothing is written and the
    ///   answer is `PreconditionFailed` (412).
    /// - The state the project already has, with `confirm`, closes what
    ///   is still open under it (the catch-up). Without `confirm` a
    ///   repeat stays the no-op it was: a stale tab or "Keep complete"
    ///   must not be forced into a cascade.
    /// - Applied: the plan's sub-Projects take the state, its tasks are
    ///   cancelled, each project where something closed gets a
    ///   `cascade_closed` entry, and the actor's own Focus entries on
    ///   what closed are removed. Only where `actor` holds a grant of
    ///   their own (`repo::editable_by`): no admin reach.
    ///
    /// **Re-opening** (S6c). A change to active, or out of a closed
    /// state, is refused (`Validation`, naming no node) while the
    /// project's home is a closed Project its actor can view
    /// (`common::cascade::reopen_refused`). `sees_all` says the actor
    /// reads every node (the admin role), and is used for this refusal
    /// only: it never widens what the cascade closes.
    ///
    /// A node that is not a Project is refused with the §3.2 reason: the
    /// route gates first, and this keeps a stray call from reaching the
    /// database's CHECK as a 500.
    ///
    /// The outcome says whether the node itself was written (a repeat of
    /// a declared state writes nothing, and the route then tells no
    /// webhook) and which other projects the cascade changed.
    async fn set_project_state(
        &self,
        id: NodeId,
        state: ProjectState,
        confirm: Option<&str>,
        actor: &ActivityActor,
        sees_all: bool,
    ) -> Result<StateOutcome, EmberTroveError>;

    /// What declaring `state` (a terminal one) on Project `id` would close
    /// for `actor`: the confirmation's content, with its token. Read on
    /// one snapshot; nothing is locked or written. `actor` is always the
    /// person's own subject, never the admin scope: the plan names only
    /// what that account can view and closes only what it holds a grant
    /// on.
    async fn cascade_preview(
        &self,
        id: NodeId,
        state: ProjectState,
        actor: &str,
    ) -> Result<CascadePlan, EmberTroveError>;

    /// The titles of `ids` that `subject` may view (`None`: an admin,
    /// every node). For showing a History entry that refers to other
    /// nodes by id (`common::cascade::presented`): a node the reader
    /// can't view is simply not in the answer.
    async fn visible_titles(
        &self,
        ids: &[Uuid],
        subject: Option<&str>,
    ) -> Result<BTreeMap<Uuid, String>, EmberTroveError>;

    /// The project statuses `owner_id` uses on their own project nodes, for
    /// the status picker's suggestions (#143). Case variants are one entry,
    /// spelled the most-used way; most-used first, at most 50.
    ///
    /// SECURITY: owner-only — never values from nodes shared with the
    /// caller or (for the admin) anyone else's. A status is free text.
    async fn project_status_counts(
        &self,
        owner_id: &str,
    ) -> Result<Vec<common::node::ProjectStatusCount>, EmberTroveError>;
}

pub struct PgNodeRepo {
    pool: PgPool,
}

impl PgNodeRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Intermediate row type for sqlx — maps DB columns with text-cast enums.
/// `project_focus` reads it too, flattened beside its own columns.
#[derive(sqlx::FromRow)]
pub(super) struct NodeRow {
    id: Uuid,
    owner_id: String,
    node_type: String,
    title: String,
    slug: String,
    body: Option<String>,
    metadata: serde_json::Value,
    status: String,
    project_status: Option<String>,
    project_state: Option<String>,
    project_state_at: Option<DateTime<Utc>>,
    pinned: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// Edge count: populated by list() via LEFT JOIN; defaults to 0 for single-row fetches.
    #[sqlx(default)]
    edge_count: i64,
}

impl NodeRow {
    pub(super) fn into_node(self) -> Result<Node, EmberTroveError> {
        Ok(Node {
            id: NodeId(self.id),
            owner_id: self.owner_id,
            node_type: parse_node_type(&self.node_type)?,
            title: self.title,
            slug: self.slug,
            body: self.body,
            metadata: self.metadata,
            status: parse_node_status(&self.status)?,
            project_status: self.project_status,
            project_state: self
                .project_state
                .as_deref()
                .map(parse_project_state)
                .transpose()?,
            project_state_at: self.project_state_at,
            tags: vec![], // populated separately if needed
            pinned: self.pinned,
            created_at: self.created_at,
            updated_at: self.updated_at,
            edge_count: u32::try_from(self.edge_count).unwrap_or(0),
        })
    }
}

/// The columns a [`NodeRow`] reads, in one place: every statement that
/// fills one names them through [`node_cols`]. They used to be written out
/// eleven times, and `FromRow` only finds a missing one when that route
/// runs (a 500 on one route, S6a).
const NODE_COLS: [&str; 14] = [
    "id",
    "owner_id",
    "node_type::text",
    "title",
    "slug",
    "body",
    "metadata",
    "status::text",
    "project_status",
    "project_state",
    "project_state_at",
    "pinned",
    "created_at",
    "updated_at",
];

/// [`NODE_COLS`] as a select list. `alias` is the table alias with its
/// dot (`"n."`), or `""`. Fixed strings only: never text from a request.
pub(super) fn node_cols(alias: &str) -> String {
    NODE_COLS
        .iter()
        .map(|col| format!("{alias}{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// SQL for "`alias`'s state is not terminal": the twin of
/// `ProjectState::is_terminal`, built from it so the two can't drift.
/// `IS DISTINCT FROM` keeps a node with no state (anything but a Project).
/// The states are the enum's own wire strings, never text from a request.
fn not_closed(alias: &str) -> String {
    ProjectState::ALL
        .iter()
        .filter(|state| state.is_terminal())
        .map(|state| {
            format!(
                "{alias}.project_state IS DISTINCT FROM '{}'",
                state.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

// ── Declaring a state, and the cascade (S6a, S6b) ───────────────────────────

/// A project the cascade changed, and whose hooks hear of it: the
/// project's owner for a state change, the owner of the cancelled tasks
/// for a task change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub id: NodeId,
    pub owner_id: String,
}

/// What [`NodeRepo::set_project_state`] did.
#[derive(Debug, Clone)]
pub struct StateOutcome {
    pub node: Node,
    /// The node itself was written (its state or the date it was declared).
    pub wrote: bool,
    /// The sub-Projects the cascade closed.
    pub closed: Vec<Touched>,
    /// The projects on which the cascade cancelled tasks, once per owner
    /// of those tasks.
    pub cancelled_on: Vec<Touched>,
}

/// Postgres' SQLSTATEs for "one of two transactions waiting on each other
/// was ended" and "a lock was not granted within `lock_timeout`".
const DEADLOCK: &str = "40P01";
const LOCK_NOT_AVAILABLE: &str = "55P03";

/// How long a state write waits for a row another transaction holds. It
/// holds the `contains` lock while it waits, and every other state write
/// and every home write queues behind that: one stuck transaction must
/// not stop them all for long.
const LOCK_TIMEOUT: &str = "SET LOCAL lock_timeout = '5s'";

fn sqlstate(e: &sqlx::Error) -> Option<&str> {
    match e {
        sqlx::Error::Database(db) => db.code().map(|c| match c {
            std::borrow::Cow::Borrowed(s) => s,
            // Compared against constants only; an owned code can't be
            // returned by reference, and none of ours is one.
            std::borrow::Cow::Owned(_) => "",
        }),
        _ => None,
    }
}

/// A failure inside the state transaction: a database error, kept whole so
/// a deadlock can be told from the rest, or an answer for the caller.
enum StateError {
    Db(sqlx::Error),
    Public(EmberTroveError),
}

impl From<sqlx::Error> for StateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl From<EmberTroveError> for StateError {
    fn from(e: EmberTroveError) -> Self {
        Self::Public(e)
    }
}

impl StateError {
    fn into_public(self) -> EmberTroveError {
        match self {
            Self::Public(e) => e,
            // Rows held by something slow (or twice in a row by a
            // deadlock): nothing was written. 409, "try again".
            Self::Db(e) if matches!(sqlstate(&e), Some(LOCK_NOT_AVAILABLE | DEADLOCK)) => {
                EmberTroveError::AlreadyExists(
                    "another change to these projects is in progress: try again".to_string(),
                )
            }
            Self::Db(e) => EmberTroveError::Internal(format!("set project state failed: {e}")),
        }
    }
}

/// The master and the Projects under it, as one actor may act on them.
struct Subtree {
    master: common::cascade::NodeIn,
    /// The master's kind: a state is a Project's.
    master_kind: NodeType,
    master_declared_at: Option<DateTime<Utc>>,
    nodes: Vec<common::cascade::NodeIn>,
    edges: Vec<(NodeId, NodeId)>,
    tasks: Vec<common::cascade::TaskIn>,
    owners: BTreeMap<NodeId, String>,
    /// Whose each task still to do is: a task's webhook goes to its own
    /// owner's hooks.
    task_owners: BTreeMap<common::id::TaskId, String>,
}

impl Subtree {
    /// The cascade's plan for declaring `target`, with its token.
    fn plan(&self, target: ProjectState) -> CascadePlan {
        let mut plan =
            common::cascade::plan(&self.master, target, &self.nodes, &self.edges, &self.tasks);
        plan.token = token_of(&plan, self.master.id);
        plan
    }

    fn touched(&self, id: NodeId) -> Touched {
        Touched {
            id,
            owner_id: self.owners.get(&id).cloned().unwrap_or_default(),
        }
    }
}

/// The token that stands for a plan: a hash of its canonical text
/// (`CascadePlan::fingerprint`). Not a secret: it is made only from what
/// the plan shows its reader, and the write decides what to close from
/// the rows under its own locks, never from the token. It can only say
/// "this is the plan I was shown".
fn token_of(plan: &CascadePlan, master: NodeId) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(plan.fingerprint(master).as_bytes()))
}

#[derive(sqlx::FromRow)]
struct SubtreeRow {
    id: Uuid,
    owner_id: String,
    node_type: String,
    title: String,
    project_state: Option<String>,
    project_state_at: Option<DateTime<Utc>>,
    editable: bool,
    viewable: bool,
}

/// Read `master` and every Project reachable from it down `contains`
/// edges, their edges, and the tasks still to do on them, as `actor` may
/// act on them. With `lock`, the node rows and the task rows are locked,
/// each in id order (the caller's transaction holds the `contains` lock
/// already, so the edges can't change).
///
/// `actor` is a person's own subject, always: `editable` is
/// `repo::editable_by` (no admin branch), and `viewable` binds it to
/// `visible_to`, whose "sees everything" case is a NULL that is never
/// passed here.
///
/// Without `walk` only the master's own row is read (and locked): a
/// change to active or parked reaches nothing below it.
async fn read_subtree(
    conn: &mut sqlx::PgConnection,
    master: NodeId,
    actor: &str,
    lock: bool,
    walk: bool,
) -> Result<Subtree, StateError> {
    use common::cascade::{NodeIn, TaskIn};
    // UNION, not UNION ALL: a loop left in older data ends (as
    // `edge::closes_loop`). The locking clause names the table; it can't
    // name the CTE.
    let rows: Vec<SubtreeRow> = sqlx::query_as(&format!(
        r#"
        WITH RECURSIVE sub(id) AS (
            SELECT $1::uuid
            UNION
            SELECT e.target_id FROM edges e JOIN sub s ON e.source_id = s.id
            WHERE e.edge_type = 'contains' AND $3
        )
        SELECT n.id, n.owner_id, n.node_type::text AS node_type, n.title,
               n.project_state, n.project_state_at,
               {editable} AS editable, {viewable} AS viewable
        FROM nodes n JOIN sub ON sub.id = n.id
        WHERE n.id = $1 OR n.node_type = 'project'
        ORDER BY n.id
        {locking}
        "#,
        editable = super::editable_by("n.id", 2),
        viewable = super::visible_to("n", 2),
        locking = if lock { "FOR NO KEY UPDATE OF n" } else { "" },
    ))
    .bind(master.0)
    .bind(actor)
    .bind(walk)
    .fetch_all(&mut *conn)
    .await?;

    let mut found = None;
    let mut nodes = Vec::new();
    let mut owners = BTreeMap::new();
    for row in rows {
        let id = NodeId(row.id);
        owners.insert(id, row.owner_id);
        let kind = parse_node_type(&row.node_type)?;
        // A Project always has a state (migration 051's CHECK); one read
        // without would be a node of another kind.
        let state = row
            .project_state
            .as_deref()
            .map(parse_project_state)
            .transpose()?;
        let node = NodeIn {
            id,
            title: row.title,
            state: state.unwrap_or(ProjectState::Active),
            editable: row.editable,
            viewable: row.viewable,
        };
        if id == master {
            found = Some((node, kind, row.project_state_at));
        } else if state.is_some() {
            nodes.push(node);
        }
    }
    let Some((master_node, master_kind, master_declared_at)) = found else {
        return Err(EmberTroveError::NotFound(format!("node {master} not found")).into());
    };

    let ids: Vec<Uuid> = std::iter::once(master.0)
        .chain(nodes.iter().map(|n| n.id.0))
        .collect();
    let edges: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT source_id, target_id FROM edges \
         WHERE edge_type = 'contains' AND source_id = ANY($1) AND target_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;

    // (id, node, title, owner): read only when the walk goes below.
    let mut tasks: Vec<(Uuid, Uuid, String, String)> = if walk {
        sqlx::query_as(&format!(
            "SELECT t.id, t.node_id, t.title, t.owner_id \
             FROM node_tasks t \
             WHERE t.node_id = ANY($1) AND t.deleted_at IS NULL AND t.status::text = ANY($2) \
             ORDER BY t.id {locking}",
            locking = if lock { "FOR UPDATE OF t" } else { "" },
        ))
        .bind(&ids)
        .bind(super::task::still_to_do())
        .fetch_all(&mut *conn)
        .await?
    } else {
        Vec::new()
    };
    // Read (and locked) in id order; shown by title.
    tasks.sort_by_key(|task| (task.2.to_lowercase(), task.0));
    let task_owners = tasks
        .iter()
        .map(|(id, _, _, owner)| (common::id::TaskId(*id), owner.clone()))
        .collect();

    Ok(Subtree {
        master: master_node,
        master_kind,
        master_declared_at,
        nodes,
        edges: edges
            .into_iter()
            .map(|(home, child)| (NodeId(home), NodeId(child)))
            .collect(),
        tasks: tasks
            .into_iter()
            .map(|(id, node, title, owner)| TaskIn {
                id: common::id::TaskId(id),
                node: NodeId(node),
                title,
                others: owner != actor,
            })
            .collect(),
        owners,
        task_owners,
    })
}

/// The 412 for a plan that is not the one its token stands for.
fn plan_changed() -> StateError {
    EmberTroveError::PreconditionFailed(
        "what is under this project changed since you were shown it: \
         look again before closing it"
            .to_string(),
    )
    .into()
}

/// History records exactly what a cascade closed (§4.1), so the cascade
/// closes exactly its plan or nothing. The rows are locked before the plan
/// is made, so a statement that changed fewer rows than planned should not
/// happen; if one does (a row changed in a way the locks don't cover), the
/// transaction is dropped and the person is asked to look again, rather
/// than a record written of something that did not happen.
fn written_all(rows: u64, planned: usize) -> Result<(), StateError> {
    if usize::try_from(rows).ok() == Some(planned) {
        Ok(())
    } else {
        Err(plan_changed())
    }
}

/// SQL for "`alias`'s state is terminal": `not_closed`'s opposite, from
/// the same rule.
pub(crate) fn is_closed(alias: &str) -> String {
    let words = ProjectState::ALL
        .iter()
        .filter(|state| state.is_terminal())
        .map(|state| format!("'{}'", state.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{alias}.project_state IN ({words})")
}

impl PgNodeRepo {
    /// One attempt at [`NodeRepo::set_project_state`], in one transaction.
    /// The node comes back without its tags.
    async fn declare_once(
        &self,
        id: NodeId,
        state: ProjectState,
        confirm: Option<&str>,
        actor: &ActivityActor,
        sees_all: bool,
    ) -> Result<StateOutcome, StateError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(LOCK_TIMEOUT).execute(&mut *tx).await?;
        // The tree first, as every write that adds a `contains` edge takes
        // it: nothing is moved under these projects while the walk below
        // reads them, and state writes apply one after another. (An edge
        // can still be removed; that only makes the plan smaller, and the
        // edges are read after the rows are locked.) The error is kept as
        // the driver's, so a timeout here is the same 409 as one on a row.
        super::edge::lock_type_raw(&mut tx, &common::edge::EdgeType::Contains).await?;
        // Then the rows, the master's among them: the History entry needs
        // the old state, and two declarations sent at once apply in turn.
        // Only a terminal state reaches below the master, so only it
        // locks what is there.
        let subtree =
            read_subtree(&mut tx, id, &actor.subject_id, true, state.is_terminal()).await?;
        if !allows(&subtree.master_kind, Capability::ProjectStatus) {
            return Err(EmberTroveError::Validation(
                Capability::ProjectStatus.refusal().to_string(),
            )
            .into());
        }
        let old = subtree.master.state;
        let changed = old != state;

        // S6c: no re-opening under a closed master the actor can view.
        // "Can view" is the one place here where the admin role counts:
        // an admin reads every node, so the refusal tells them nothing,
        // and without it they alone could re-open under a closed master.
        // What the cascade *closes* never looks at `sees_all`.
        if changed && !state.is_terminal() {
            let master_closed: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS (SELECT 1 FROM edges e JOIN nodes p ON p.id = e.source_id \
                 WHERE e.target_id = $1 AND e.edge_type = 'contains' \
                   AND p.node_type = 'project' AND {closed} \
                   AND ($3::bool OR {visible}))",
                closed = is_closed("p"),
                visible = super::visible_to("p", 2),
            ))
            .bind(id.0)
            .bind(&actor.subject_id)
            .bind(sees_all)
            .fetch_one(&mut *tx)
            .await?;
            if common::cascade::reopen_refused(old, state, master_closed) {
                return Err(EmberTroveError::Validation(
                    common::cascade::MASTER_IS_CLOSED.to_string(),
                )
                .into());
            }
        }

        // S6b: what this declaration closes, from the rows just locked.
        let plan = state.is_terminal().then(|| subtree.plan(state));
        let cascade = match (&plan, confirm) {
            (Some(plan), Some(token)) if plan.closes_something() => {
                if token != plan.token {
                    return Err(plan_changed());
                }
                Some(plan)
            }
            // A change that closes something is never made unconfirmed.
            // (A repeat of the state it has, unconfirmed, cascades
            // nothing: it stays the no-op it was.)
            (Some(plan), None) if changed && plan.closes_something() => {
                return Err(EmberTroveError::PreconditionFailed(format!(
                    "closing this project also closes {} sub-project(s) and cancels {} \
                     task(s), which must be confirmed first (if this page offers no \
                     confirmation, reload it)",
                    plan.projects.len(),
                    plan.task_count()
                ))
                .into());
            }
            // Nothing to close: a token, if one was sent (a retry after
            // the cascade already ran), is beside the point.
            _ => None,
        };

        let wrote = changed || subtree.master_declared_at.is_none();
        if wrote {
            // The state it already has, already declared, writes nothing:
            // `updated_at` is the version an open editor saves against.
            // `clock_timestamp()`, not `now()`: `now()` is the transaction's
            // start, before the wait for the locks above, so of two
            // declarations sent at once the one that landed last could
            // carry the earlier date (the History entry's rule, #208).
            sqlx::query(
                "UPDATE nodes SET project_state = $2, project_state_at = clock_timestamp() \
                 WHERE id = $1",
            )
            .bind(id.0)
            .bind(state.as_str())
            .execute(&mut *tx)
            .await?;
        }
        if changed {
            let meta = actor.metadata_with(serde_json::json!({
                "from": old.as_str(),
                "to": state.as_str(),
            }));
            record_in(
                &mut tx,
                id,
                &actor.subject_id,
                ActivityAction::ProjectStateChanged,
                meta,
            )
            .await?;
        }
        if state.is_terminal() {
            // Whether or not the state changed: a stale tab closing a
            // project again must still leave no entry of its person's.
            super::project_focus::remove_on(&mut tx, &actor.subject_id, id).await?;
        }

        let (mut closed, mut cancelled_on) = (Vec::new(), Vec::new());
        if let Some(plan) = cascade {
            let closing: Vec<Uuid> = plan.projects.iter().map(|p| p.id.0).collect();
            if !closing.is_empty() {
                let done = sqlx::query(&format!(
                    "UPDATE nodes n SET project_state = $2, project_state_at = clock_timestamp() \
                     WHERE n.id = ANY($1) AND NOT {closed}",
                    closed = is_closed("n"),
                ))
                .bind(&closing)
                .bind(state.as_str())
                .execute(&mut *tx)
                .await?;
                written_all(done.rows_affected(), closing.len())?;
            }
            for project in &plan.projects {
                let meta = actor.metadata_with(serde_json::json!({
                    "from": project.from.as_str(),
                    "to": state.as_str(),
                }));
                record_in(
                    &mut tx,
                    project.id,
                    &actor.subject_id,
                    ActivityAction::ProjectStateChanged,
                    meta,
                )
                .await?;
                // The actor's own entry, as for the master. Nobody else's.
                super::project_focus::remove_on(&mut tx, &actor.subject_id, project.id).await?;
                closed.push(subtree.touched(project.id));
            }

            let task_ids: Vec<Uuid> = plan
                .tasks
                .iter()
                .flat_map(|group| group.tasks.iter().map(|task| task.id.0))
                .collect();
            let cancelled = super::task::cancel_open_on(&mut tx, &task_ids).await?;
            written_all(cancelled, task_ids.len())?;

            // The record of exactly what closed (§4.1), on each project
            // where something did: the master's lists the sub-Projects, by
            // id; every entry lists that project's own cancelled tasks.
            // The master's own entry exists when it has something to
            // say: a sub-Project closed, or a task of its own cancelled.
            let closed_ids: Vec<NodeId> = plan.projects.iter().map(|p| p.id).collect();
            let mut recorded_on = plan.touched_nodes();
            if !closed_ids.is_empty() {
                recorded_on.insert(id);
            }
            for node in recorded_on {
                let own_tasks = plan
                    .tasks
                    .iter()
                    .find(|group| group.node_id == node)
                    .map_or(&[][..], |group| group.tasks.as_slice());
                let (root, subs) = if node == id {
                    (None, closed_ids.as_slice())
                } else {
                    (Some(id), &[][..])
                };
                let meta =
                    actor.metadata_with(common::cascade::recorded(state, root, subs, own_tasks));
                record_in(
                    &mut tx,
                    node,
                    &actor.subject_id,
                    ActivityAction::CascadeClosed,
                    meta,
                )
                .await?;
            }
            // One per project and task owner: a task's event goes to its
            // own owner's hooks, as every task event does.
            let pairs: BTreeSet<(NodeId, &str)> = plan
                .tasks
                .iter()
                .flat_map(|group| {
                    group.tasks.iter().filter_map(|task| {
                        let owner = subtree.task_owners.get(&task.id)?;
                        Some((group.node_id, owner.as_str()))
                    })
                })
                .collect();
            cancelled_on = pairs
                .into_iter()
                .map(|(id, owner)| Touched {
                    id,
                    owner_id: owner.to_string(),
                })
                .collect();
        }

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {cols} FROM nodes WHERE id = $1",
            cols = node_cols(""),
        ))
        .bind(id.0)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(StateOutcome {
            node: row.into_node()?,
            wrote,
            closed,
            cancelled_on,
        })
    }
}

/// Row type for batch tag fetch — includes the owning node_id.
#[derive(sqlx::FromRow)]
struct NodeTagRow {
    node_id: Uuid,
    id: Uuid,
    owner_id: String,
    name: String,
    color: String,
    created_at: DateTime<Utc>,
}

/// How many links each of `node_ids` has whose other end `subject` can see
/// (`None` is an admin: every link). Only the page's nodes are counted; the
/// list used to aggregate every edge in the database on each call.
///
/// SECURITY (ONTOLOGY §14: never show a count of hidden items): a count of
/// all links told an editor of a shared node whether its owner had a node
/// with a title they guessed. They wrote `[[the guess]]` into the body and
/// the count rose exactly when it resolved.
async fn visible_link_counts(
    pool: &PgPool,
    node_ids: &[Uuid],
    subject: Option<&str>,
) -> Result<HashMap<Uuid, u32>, EmberTroveError> {
    if node_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, i64)> = sqlx::query_as(&format!(
        "SELECT e.this, COUNT(*) FROM ( \
             SELECT source_id AS this, target_id AS other FROM edges WHERE source_id = ANY($1) \
             UNION ALL \
             SELECT target_id AS this, source_id AS other FROM edges WHERE target_id = ANY($1) \
         ) e JOIN nodes o ON o.id = e.other \
         WHERE {} GROUP BY e.this",
        super::visible_to("o", 2)
    ))
    .bind(node_ids)
    .bind(subject)
    .fetch_all(pool)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("count node links failed: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|(id, n)| (id, u32::try_from(n).unwrap_or(u32::MAX)))
        .collect())
}

/// Fetch all tags for a slice of node IDs in one query.
/// Returns a map of node_id → Vec<Tag>.
async fn fetch_tags_for_nodes<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    node_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Tag>>, EmberTroveError> {
    if node_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query_as::<_, NodeTagRow>(
        r#"
        SELECT nt.node_id, t.id, t.owner_id, t.name, t.color, t.created_at
        FROM tags t
        JOIN node_tags nt ON nt.tag_id = t.id
        WHERE nt.node_id = ANY($1)
        ORDER BY t.name
        "#,
    )
    .bind(node_ids)
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("fetch tags for nodes failed: {e}")))?;

    let mut map: HashMap<Uuid, Vec<Tag>> = HashMap::new();
    for row in rows {
        map.entry(row.node_id).or_default().push(Tag {
            id: TagId(row.id),
            owner_id: row.owner_id,
            name: row.name,
            color: row.color,
            created_at: row.created_at,
        });
    }
    Ok(map)
}

fn parse_node_type(s: &str) -> Result<NodeType, EmberTroveError> {
    super::parse_wire(s, "node_type", NodeType::parse)
}

fn parse_node_status(s: &str) -> Result<NodeStatus, EmberTroveError> {
    super::parse_wire(s, "node_status", NodeStatus::parse)
}

fn parse_project_state(s: &str) -> Result<ProjectState, EmberTroveError> {
    super::parse_wire(s, "project_state", ProjectState::parse)
}

/// Insert a node (`NodeRepo::create`), on any executor, so capture
/// conversion (#234) can do it inside its transaction. A taken slug or
/// title is `AlreadyExists` (409).
pub(crate) async fn insert_node<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    owner_id: &str,
    req: &CreateNodeRequest,
) -> Result<Node, EmberTroveError> {
    let slug = slugify(&req.title);
    let node_type_str = req.node_type.as_str();
    let status_str = req.status.as_ref().unwrap_or(&NodeStatus::Draft).as_str();
    // A new Project is active and nobody has declared that yet (S6a); no
    // other kind has a state. The database refuses a row that gets this
    // wrong (`nodes_project_state_only_on_projects`).
    let state = (req.node_type == NodeType::Project).then_some(ProjectState::Active.as_str());

    let row = sqlx::query_as::<_, NodeRow>(&format!(
        r#"
        INSERT INTO nodes (owner_id, node_type, title, slug, body, metadata, status,
                           project_status, project_state)
        VALUES ($1, $2::node_type, $3, $4, $5, $6, $7::node_status, $8, $9)
        RETURNING {cols}
        "#,
        cols = node_cols(""),
    ))
    .bind(owner_id)
    .bind(node_type_str)
    .bind(&req.title)
    .bind(&slug)
    .bind(&req.body)
    .bind(&req.metadata)
    .bind(status_str)
    .bind(&req.project_status)
    .bind(state)
    .fetch_one(ex)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.constraint() == Some("nodes_slug_key") => {
            EmberTroveError::AlreadyExists(format!("slug already exists: {slug}"))
        }
        sqlx::Error::Database(db_err)
            if db_err.constraint() == Some("nodes_owner_title_unique") =>
        {
            EmberTroveError::AlreadyExists(format!("a node named \"{}\" already exists", req.title))
        }
        _ => EmberTroveError::Internal(format!("create node failed: {e}")),
    })?;

    row.into_node()
}

/// Every node with its tags, all owners. A free function so the backup can run it on its one
/// snapshot transaction (`backup::read_snapshot`).
pub(crate) async fn list_all_on(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<Node>, EmberTroveError> {
    let rows = sqlx::query_as::<_, NodeRow>(&format!(
        "SELECT {cols} FROM nodes ORDER BY created_at ASC",
        cols = node_cols(""),
    ))
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list_all nodes failed: {e}")))?;

    let mut nodes = rows
        .into_iter()
        .map(NodeRow::into_node)
        .collect::<Result<Vec<_>, _>>()?;

    let node_ids: Vec<Uuid> = nodes.iter().map(|n| n.id.0).collect();
    let mut tag_map = fetch_tags_for_nodes(&mut *conn, &node_ids).await?;
    for node in &mut nodes {
        node.tags = tag_map.remove(&node.id.0).unwrap_or_default();
    }

    Ok(nodes)
}

/// The `ORDER BY` terms for a list order. Fixed strings only.
fn order_by(sort: NodeSort) -> &'static str {
    match sort {
        NodeSort::ModifiedDesc => "n.updated_at DESC, n.id",
        NodeSort::ModifiedAsc => "n.updated_at ASC, n.id",
        NodeSort::TitleAsc => "lower(n.title) ASC, n.id",
        NodeSort::TitleDesc => "lower(n.title) DESC, n.id",
    }
}

#[async_trait]
impl NodeRepo for PgNodeRepo {
    async fn create(
        &self,
        owner_id: &str,
        req: CreateNodeRequest,
    ) -> Result<Node, EmberTroveError> {
        insert_node(&self.pool, owner_id, &req).await
    }

    async fn get(&self, id: NodeId) -> Result<Node, EmberTroveError> {
        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {cols} FROM nodes WHERE id = $1",
            cols = node_cols(""),
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get node failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("node {id} not found")))?;

        let mut node = row.into_node()?;
        let mut tag_map = fetch_tags_for_nodes(&self.pool, &[id.0]).await?;
        node.tags = tag_map.remove(&id.0).unwrap_or_default();
        Ok(node)
    }

    async fn get_by_slug(&self, slug: &str) -> Result<Node, EmberTroveError> {
        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {cols} FROM nodes WHERE slug = $1",
            cols = node_cols(""),
        ))
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get node by slug failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("node with slug '{slug}' not found")))?;

        row.into_node()
    }

    async fn list(&self, params: NodeListParams) -> Result<(Vec<Node>, u64), EmberTroveError> {
        let page = params.page.unwrap_or(1).max(1);
        let per_page = params.per_page.unwrap_or(50).min(200);
        // Saturating: an absurd ?page= must not overflow.
        let offset = page.saturating_sub(1).saturating_mul(per_page);

        // Build dynamic count query with optional filters.
        let mut count_sql = String::from("SELECT COUNT(*) FROM nodes n");
        let mut data_sql = format!(
            "SELECT {cols}, 0::bigint AS edge_count FROM nodes n",
            cols = node_cols("n."),
        );

        let mut conditions: Vec<String> = Vec::new();
        let mut param_idx = 1i32;

        if params.tag_id.is_some() {
            count_sql.push_str(" JOIN node_tags nt ON nt.node_id = n.id");
            data_sql.push_str(" JOIN node_tags nt ON nt.node_id = n.id");
            conditions.push(format!("nt.tag_id = ${param_idx}"));
            param_idx += 1;
        }

        if params.node_type.is_some() {
            conditions.push(format!("n.node_type = ${param_idx}::node_type"));
            param_idx += 1;
        }

        if params.status.is_some() {
            conditions.push(format!("n.status = ${param_idx}::node_status"));
            param_idx += 1;
        } else if !params.include_archived {
            // Default: hide archived nodes unless the caller explicitly opts in.
            conditions.push("n.status <> 'archived'::node_status".to_owned());
        }

        if params.project_status.is_some() {
            conditions.push(format!("lower(n.project_status) = lower(${param_idx})"));
            param_idx += 1;
        }

        // S6a: a node of another kind has no state, so it never matches.
        if params.project_state.is_some() {
            conditions.push(format!("n.project_state = ${param_idx}"));
            param_idx += 1;
        }
        if params.exclude_closed {
            // The dashboard: in SQL, so closed projects don't use up the
            // page.
            conditions.push(not_closed("n"));
        }

        if params.owner_id.is_some() {
            conditions.push(format!("n.owner_id = ${param_idx}"));
            param_idx += 1;
        }

        if params.subject_id.is_some() {
            // Restrict to nodes the caller owns or holds any permission on.
            // `param_idx` counts this query's own filters: a handful.
            conditions.push(super::visible_among_many(
                "n",
                u8::try_from(param_idx).unwrap_or(u8::MAX),
            ));
            param_idx += 1;
        }

        if !conditions.is_empty() {
            let where_clause = " WHERE ".to_owned() + &conditions.join(" AND ");
            count_sql.push_str(&where_clause);
            data_sql.push_str(&where_clause);
        }

        // Build count query with bindings.
        let mut count_query = sqlx::query_scalar(&count_sql);
        if let Some(ref tag_id) = params.tag_id {
            count_query = count_query.bind(tag_id.0);
        }
        if let Some(ref node_type) = params.node_type {
            count_query = count_query.bind(node_type.as_str());
        }
        if let Some(ref status) = params.status {
            count_query = count_query.bind(status.as_str());
        }
        if let Some(ref ps) = params.project_status {
            count_query = count_query.bind(ps.as_str());
        }
        if let Some(state) = params.project_state {
            count_query = count_query.bind(state.as_str());
        }
        if let Some(ref owner_id) = params.owner_id {
            count_query = count_query.bind(owner_id.as_str());
        }
        if let Some(ref subject_id) = params.subject_id {
            count_query = count_query.bind(subject_id.as_str());
        }

        let total: i64 = count_query
            .fetch_one(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("count query failed: {e}")))?;

        // Get paginated data. Pinned nodes always sort first within any
        // order. The order is one of four fixed strings (`order_by`), never
        // text from the request, and ends on the id so that pages can't
        // overlap or skip when two rows tie (#257).
        data_sql.push_str(&format!(
            " ORDER BY n.pinned DESC, {} LIMIT ${param_idx} OFFSET ${}",
            order_by(params.sort),
            param_idx + 1
        ));

        let mut query = sqlx::query_as::<_, NodeRow>(&data_sql);

        if let Some(ref tag_id) = params.tag_id {
            query = query.bind(tag_id.0);
        }
        if let Some(ref node_type) = params.node_type {
            query = query.bind(node_type.as_str());
        }
        if let Some(ref status) = params.status {
            query = query.bind(status.as_str());
        }
        if let Some(ref ps) = params.project_status {
            query = query.bind(ps.as_str());
        }
        if let Some(state) = params.project_state {
            query = query.bind(state.as_str());
        }
        if let Some(ref owner_id) = params.owner_id {
            query = query.bind(owner_id.as_str());
        }
        if let Some(ref subject_id) = params.subject_id {
            query = query.bind(subject_id.as_str());
        }

        query = query.bind(per_page as i64).bind(offset as i64);

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("list nodes failed: {e}")))?;

        let mut nodes = rows
            .into_iter()
            .map(NodeRow::into_node)
            .collect::<Result<Vec<_>, _>>()?;

        // Batch-fetch tags for all returned nodes in a single query.
        let node_ids: Vec<Uuid> = nodes.iter().map(|n| n.id.0).collect();
        let mut tag_map = fetch_tags_for_nodes(&self.pool, &node_ids).await?;
        // And each one's link count, as this reader may see it.
        let mut link_counts =
            visible_link_counts(&self.pool, &node_ids, params.subject_id.as_deref()).await?;
        for node in &mut nodes {
            node.tags = tag_map.remove(&node.id.0).unwrap_or_default();
            node.edge_count = link_counts.remove(&node.id.0).unwrap_or(0);
        }

        Ok((nodes, total as u64))
    }

    async fn update(
        &self,
        id: NodeId,
        req: UpdateNodeRequest,
        actor: &ActivityActor,
    ) -> Result<Node, EmberTroveError> {
        let status_str = req.status.as_ref().map(|v| v.as_str());
        let db_err = |e: sqlx::Error| EmberTroveError::Internal(format!("update node failed: {e}"));

        let mut tx = self.pool.begin().await.map_err(db_err)?;
        // Lock the row and read the status it had: the History entry needs
        // the old value, and the lock makes "no row" mean 404 and a failed
        // precondition below mean 412, with no second lookup. NO KEY UPDATE
        // is the lock an UPDATE that leaves the key alone takes anyway, so
        // it doesn't block FK inserts (tags, attachments, activity) meanwhile.
        let Some(old_status) = sqlx::query_scalar::<_, Option<String>>(
            "SELECT project_status FROM nodes WHERE id = $1 FOR NO KEY UPDATE",
        )
        .bind(id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        else {
            return Err(EmberTroveError::NotFound(format!("node {id} not found")));
        };

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            r#"
            UPDATE nodes SET
                title    = COALESCE($2, title),
                body     = COALESCE($3, body),
                metadata = COALESCE($4, metadata),
                status   = COALESCE($5::node_status, status),
                -- Three states (#143): $6 false = leave it; true = set to $7,
                -- where NULL clears.
                project_status = CASE WHEN $6 THEN $7 ELSE project_status END
            -- #161: with a precondition, apply only to the version the
            -- client saw. The row is locked above, so a concurrent save
            -- waits for this one and then sees the new version: of two
            -- saves sent on one version only the first applies.
            WHERE id = $1 AND ($8::timestamptz IS NULL OR updated_at = $8)
            RETURNING {cols}
            "#,
            cols = node_cols(""),
        ))
        .bind(id.0)
        .bind(&req.title)
        .bind(&req.body)
        .bind(&req.metadata)
        .bind(status_str)
        .bind(req.project_status.is_some())
        .bind(req.project_status.clone().flatten())
        .bind(req.expected_updated_at)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err)
                if db_err.constraint() == Some("nodes_owner_title_unique") =>
            {
                EmberTroveError::AlreadyExists(format!(
                    "a node named \"{}\" already exists",
                    req.title.as_deref().unwrap_or("")
                ))
            }
            _ => EmberTroveError::Internal(format!("update node failed: {e}")),
        })?;
        // The row exists and is locked, so no row here can only be a failed
        // precondition. Only a caller that passed authorization reaches the
        // repo, so telling 412 from 404 leaks nothing.
        let Some(row) = row else {
            return Err(EmberTroveError::PreconditionFailed(
                "the node changed since it was loaded".to_string(),
            ));
        };

        if row.project_status != old_status {
            let meta = actor.metadata_with(serde_json::json!({
                "from": old_status,
                "to": row.project_status,
            }));
            record_in(
                &mut tx,
                id,
                &actor.subject_id,
                ActivityAction::StatusChanged,
                meta,
            )
            .await?;
        }

        tx.commit().await.map_err(db_err)?;
        row.into_node()
    }

    async fn delete(&self, id: NodeId) -> Result<(), EmberTroveError> {
        // Its attachment rows cascade away, and the `attachments_queue_object`
        // trigger queues their files for deletion from storage (#239).
        let result = sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete node failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!("node {id} not found")));
        }

        Ok(())
    }

    async fn deletion_preview(
        &self,
        ids: &[NodeId],
        caller: &str,
    ) -> Result<NodeDeletionPreview, EmberTroveError> {
        // These are the rows `DELETE FROM nodes` cascades to. The trash
        // (`deleted_at`) goes too, but it is already deleted to its owner,
        // so it is not counted.
        let ids: Vec<Uuid> = ids.iter().map(|id| id.0).collect();
        let row: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
               (SELECT COUNT(*) FROM nodes WHERE id = ANY($1)),
               (SELECT COUNT(*) FROM node_tasks
                 WHERE node_id = ANY($1) AND deleted_at IS NULL),
               (SELECT COUNT(*) FROM node_tasks
                 WHERE node_id = ANY($1) AND deleted_at IS NULL AND owner_id <> $2),
               (SELECT COUNT(*) FROM node_notes
                 WHERE node_id = ANY($1) AND deleted_at IS NULL),
               (SELECT COUNT(*) FROM node_notes
                 WHERE node_id = ANY($1) AND deleted_at IS NULL AND owner_id <> $2),
               (SELECT COUNT(*) FROM attachments WHERE node_id = ANY($1)),
               (SELECT COUNT(*) FROM project_briefs WHERE node_id = ANY($1))",
        )
        .bind(&ids)
        .bind(caller)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("node deletion preview failed: {e}")))?;
        let n = |v: i64| u64::try_from(v).unwrap_or(0);
        Ok(NodeDeletionPreview {
            nodes: n(row.0),
            tasks: n(row.1),
            others_tasks: n(row.2),
            notes: n(row.3),
            others_notes: n(row.4),
            files: n(row.5),
            briefs: n(row.6),
        })
    }

    async fn neighbors(
        &self,
        id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<Node>, EmberTroveError> {
        // SECURITY: the route checks access to node `id` only; every linked
        // node must also be visible to the caller, or a link (e.g. a
        // pre-hotfix cross-owner wiki link) would expose its full body.
        let rows = sqlx::query_as::<_, NodeRow>(&format!(
            r#"
            SELECT {cols}
            FROM nodes n
            -- `related_to` has no direction (#226): its other end is a
            -- neighbour whichever way it is stored.
            JOIN edges e
              ON (e.source_id = $1 AND e.target_id = n.id)
              OR (e.edge_type = 'related_to' AND e.target_id = $1 AND e.source_id = n.id)
            WHERE {visible}
            ORDER BY n.title
            "#,
            cols = node_cols("n."),
            visible = super::visible_to("n", 2),
        ))
        .bind(id.0)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("neighbors query failed: {e}")))?;

        rows.into_iter().map(NodeRow::into_node).collect()
    }

    async fn backlinks(
        &self,
        id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<Node>, EmberTroveError> {
        // SECURITY: the route checks access to node `id` only; every linked
        // node must also be visible to the caller, or a link (e.g. a
        // pre-hotfix cross-owner wiki link) would expose its full body.
        let rows = sqlx::query_as::<_, NodeRow>(&format!(
            r#"
            SELECT {cols}
            FROM nodes n
            -- `related_to` has no direction (#226): it links back too.
            JOIN edges e
              ON (e.target_id = $1 AND e.source_id = n.id)
              OR (e.edge_type = 'related_to' AND e.source_id = $1 AND e.target_id = n.id)
            WHERE {visible}
            ORDER BY n.title
            "#,
            cols = node_cols("n."),
            visible = super::visible_to("n", 2),
        ))
        .bind(id.0)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("backlinks query failed: {e}")))?;

        rows.into_iter().map(NodeRow::into_node).collect()
    }

    async fn lookup_titles(
        &self,
        subject: Option<&str>,
        q: &str,
        limit: i64,
        node_type: Option<&NodeType>,
    ) -> Result<Vec<common::node::NodeLookupHit>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct HitRow {
            id: Uuid,
            title: String,
            node_type: String,
        }

        let q = q.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = super::escape_like(&q.to_lowercase());
        // SECURITY: every value is a bind parameter; `visible_to` only
        // splices compile-time literals.
        let sql = format!(
            r#"
            SELECT n.id, n.title, n.node_type::text AS node_type
            FROM nodes n
            WHERE {visible}
              AND lower(n.title) LIKE ('%' || $2 || '%') ESCAPE '\'
              AND ($5::text IS NULL OR n.node_type = $5::node_type)
            ORDER BY (lower(n.title) = lower($3)) DESC,
                     (lower(n.title) LIKE ($2 || '%') ESCAPE '\') DESC,
                     lower(n.title), n.id
            LIMIT $4
            "#,
            visible = super::visible_to("n", 1),
        );
        let rows = sqlx::query_as::<_, HitRow>(&sql)
            .bind(subject)
            .bind(&pattern)
            .bind(q)
            .bind(limit)
            .bind(node_type.map(NodeType::as_str))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("lookup_titles failed: {e}")))?;
        rows.into_iter()
            .map(|r| {
                Ok(common::node::NodeLookupHit {
                    id: NodeId(r.id),
                    title: r.title,
                    node_type: parse_node_type(&r.node_type)?,
                })
            })
            .collect()
    }

    async fn list_titles(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<NodeTitleEntry>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct TitleRow {
            id: Uuid,
            title: String,
            slug: String,
        }

        // Admin (`None`): every node. Otherwise the nodes the caller owns
        // or was granted, by the one rule (`visible_among_many`; #281, 3.2:
        // this was a third spelling of it, a LEFT JOIN with DISTINCT).
        let sql = format!(
            "SELECT n.id, n.title, n.slug FROM nodes n WHERE {visible} ORDER BY n.title",
            visible = super::visible_among_many("n", 1),
        );
        let rows = sqlx::query_as::<_, TitleRow>(&sql)
            .bind(subject)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("list_titles failed: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| NodeTitleEntry {
                id: NodeId(r.id),
                title: r.title,
                slug: r.slug,
            })
            .collect())
    }

    async fn find_ids_by_titles(
        &self,
        owner_id: &str,
        lowered_titles: &[String],
    ) -> Result<HashMap<String, NodeId>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct TitleIdRow {
            id: Uuid,
            lt: String,
        }

        // One winner per title: the owner's own node first ((owner_id,
        // lower(title)) is unique), else the lowest-id shared node.
        let rows = sqlx::query_as::<_, TitleIdRow>(&format!(
            "SELECT DISTINCT ON (LOWER(n.title)) n.id, LOWER(n.title) AS lt
             FROM nodes n
             WHERE LOWER(n.title) = ANY($2)
               AND {visible}
             ORDER BY LOWER(n.title), (n.owner_id = $1) DESC, n.id",
            visible = super::visible_to("n", 1),
        ))
        .bind(owner_id)
        .bind(lowered_titles)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("find_ids_by_titles failed: {e}")))?;

        Ok(rows.into_iter().map(|r| (r.lt, NodeId(r.id))).collect())
    }

    async fn list_all_for_owner(&self, owner_id: &str) -> Result<Vec<Node>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {cols} FROM nodes WHERE owner_id = $1 ORDER BY created_at ASC",
            cols = node_cols(""),
        ))
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list_all_for_owner failed: {e}")))?;

        let mut nodes = rows
            .into_iter()
            .map(NodeRow::into_node)
            .collect::<Result<Vec<_>, _>>()?;

        let node_ids: Vec<Uuid> = nodes.iter().map(|n| n.id.0).collect();
        let mut tag_map = fetch_tags_for_nodes(&self.pool, &node_ids).await?;
        for node in &mut nodes {
            node.tags = tag_map.remove(&node.id.0).unwrap_or_default();
        }

        Ok(nodes)
    }

    async fn list_all(&self) -> Result<Vec<Node>, EmberTroveError> {
        let mut conn =
            self.pool.acquire().await.map_err(|e| {
                EmberTroveError::Internal(format!("acquire connection failed: {e}"))
            })?;
        list_all_on(&mut conn).await
    }

    async fn set_pinned(&self, id: NodeId, pinned: bool) -> Result<Node, EmberTroveError> {
        let row = sqlx::query_as::<_, NodeRow>(&format!(
            r#"
            UPDATE nodes
            SET pinned = $2,
                updated_at = NOW()
            WHERE id = $1
            RETURNING {cols}
            "#,
            cols = node_cols(""),
        ))
        .bind(id.0)
        .bind(pinned)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("set_pinned failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("node {id} not found")))?;

        let mut node = NodeRow::into_node(row)?;
        let mut tag_map = fetch_tags_for_nodes(&self.pool, &[node.id.0]).await?;
        node.tags = tag_map.remove(&node.id.0).unwrap_or_default();
        Ok(node)
    }

    async fn set_project_state(
        &self,
        id: NodeId,
        state: ProjectState,
        confirm: Option<&str>,
        actor: &ActivityActor,
        sees_all: bool,
    ) -> Result<StateOutcome, EmberTroveError> {
        // The cascade locks many rows, and so do other writers (the
        // account purge, a task reorder). They take them in one order
        // (nodes by id, then tasks by id), but a writer this code doesn't
        // know may not. Postgres ends such a meeting by failing one side:
        // nothing was written, so that side simply goes again, once.
        let mut attempt = self.declare_once(id, state, confirm, actor, sees_all).await;
        if matches!(&attempt, Err(StateError::Db(e)) if sqlstate(e) == Some(DEADLOCK)) {
            attempt = self.declare_once(id, state, confirm, actor, sees_all).await;
        }
        let outcome = attempt.map_err(StateError::into_public)?;

        let mut node = outcome.node;
        let mut tag_map = fetch_tags_for_nodes(&self.pool, &[id.0]).await?;
        node.tags = tag_map.remove(&id.0).unwrap_or_default();
        Ok(StateOutcome { node, ..outcome })
    }

    async fn cascade_preview(
        &self,
        id: NodeId,
        state: ProjectState,
        actor: &str,
    ) -> Result<CascadePlan, EmberTroveError> {
        let db_err =
            |e: sqlx::Error| EmberTroveError::Internal(format!("cascade preview failed: {e}"));
        // One snapshot for the nodes, the edges and the tasks.
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let subtree = read_subtree(&mut tx, id, actor, false, true)
            .await
            .map_err(StateError::into_public)?;
        tx.commit().await.map_err(db_err)?;
        Ok(subtree.plan(state))
    }

    async fn visible_titles(
        &self,
        ids: &[Uuid],
        subject: Option<&str>,
    ) -> Result<BTreeMap<Uuid, String>, EmberTroveError> {
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let rows: Vec<(Uuid, String)> = sqlx::query_as(&format!(
            "SELECT n.id, n.title FROM nodes n WHERE n.id = ANY($1) AND {visible}",
            visible = super::visible_to("n", 2),
        ))
        .bind(ids)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("visible titles failed: {e}")))?;
        Ok(rows.into_iter().collect())
    }

    async fn project_status_counts(
        &self,
        owner_id: &str,
    ) -> Result<Vec<common::node::ProjectStatusCount>, EmberTroveError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            status: String,
            count: i64,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT (array_agg(project_status ORDER BY c DESC, project_status))[1] AS status,
                   SUM(c)::bigint AS count
            FROM (
                SELECT project_status, COUNT(*) AS c
                FROM nodes
                WHERE owner_id = $1
                  AND node_type = 'project'::node_type
                  AND project_status IS NOT NULL
                GROUP BY project_status
            ) spelled
            GROUP BY lower(project_status)
            ORDER BY count DESC, status
            LIMIT 50
            "#,
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("project status counts failed: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|r| common::node::ProjectStatusCount {
                status: r.status,
                count: u32::try_from(r.count).unwrap_or(u32::MAX),
            })
            .collect())
    }

    async fn area_for_nodes(
        &self,
        node_ids: &[NodeId],
        subject: Option<&str>,
    ) -> Result<Vec<(NodeId, NodeId, String)>, EmberTroveError> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = node_ids.iter().map(|n| n.0).collect();

        // ROW_NUMBER picks the oldest `contains` edge per project so
        // multiple "areas containing this project" collapse to one
        // parent.  Stable across runs (created_at + edge id tiebreak).
        #[derive(sqlx::FromRow)]
        struct Row {
            project_id: Uuid,
            area_id: Uuid,
            area_title: String,
        }
        let a_visible = super::visible_to("a", 2);
        let rows = sqlx::query_as::<_, Row>(&format!(
            r#"
            WITH ranked AS (
                SELECT
                    e.target_id AS project_id,
                    a.id        AS area_id,
                    a.title     AS area_title,
                    ROW_NUMBER() OVER (
                        PARTITION BY e.target_id
                        ORDER BY e.created_at ASC, e.id ASC
                    ) AS rn
                FROM edges e
                JOIN nodes a ON a.id = e.source_id
                WHERE e.target_id = ANY($1)
                  AND e.edge_type::text = 'contains'
                  AND a.node_type::text = 'area'
                  AND {a_visible}
            )
            SELECT project_id, area_id, area_title FROM ranked WHERE rn = 1
            "#
        ))
        .bind(&ids)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("area_for_nodes failed: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| (NodeId(r.project_id), NodeId(r.area_id), r.area_title))
            .collect())
    }
}
