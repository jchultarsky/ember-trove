use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    activity::ActivityAction,
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
use std::collections::HashMap;

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

    /// Declare a Project's state (S6a; ONTOLOGY §4.1). One transaction:
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
    /// A node that is not a Project is refused with the §3.2 reason: the
    /// route gates first, and this keeps a stray call from reaching the
    /// database's CHECK as a 500.
    async fn set_project_state(
        &self,
        id: NodeId,
        state: ProjectState,
        actor: &ActivityActor,
    ) -> Result<Node, EmberTroveError>;

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
        actor: &ActivityActor,
    ) -> Result<Node, EmberTroveError> {
        let db_err =
            |e: sqlx::Error| EmberTroveError::Internal(format!("set project state failed: {e}"));

        let mut tx = self.pool.begin().await.map_err(db_err)?;
        // Lock the row and read what it had (as `update` does): the History
        // entry needs the old state, and two declarations sent at once
        // then apply one after the other.
        let Some((kind, old, declared_at)) =
            sqlx::query_as::<_, (String, Option<String>, Option<DateTime<Utc>>)>(
                "SELECT node_type::text, project_state, project_state_at \
                 FROM nodes WHERE id = $1 FOR NO KEY UPDATE",
            )
            .bind(id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?
        else {
            return Err(EmberTroveError::NotFound(format!("node {id} not found")));
        };
        if !allows(&parse_node_type(&kind)?, Capability::ProjectStatus) {
            return Err(EmberTroveError::Validation(
                Capability::ProjectStatus.refusal().to_string(),
            ));
        }

        let changed = old.as_deref() != Some(state.as_str());
        if changed || declared_at.is_none() {
            // The state it already has, already declared, writes nothing:
            // `updated_at` is the version an open editor saves against.
            // `clock_timestamp()`, not `now()`: `now()` is the transaction's
            // start, before the wait for the lock above, so of two
            // declarations sent at once the one that landed last could
            // carry the earlier date (the History entry's rule, #208).
            sqlx::query(
                "UPDATE nodes SET project_state = $2, project_state_at = clock_timestamp() \
                 WHERE id = $1",
            )
            .bind(id.0)
            .bind(state.as_str())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        if changed {
            let meta = actor.metadata_with(serde_json::json!({
                "from": old,
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

        let row = sqlx::query_as::<_, NodeRow>(&format!(
            "SELECT {cols} FROM nodes WHERE id = $1",
            cols = node_cols(""),
        ))
        .bind(id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;

        let mut node = row.into_node()?;
        let mut tag_map = fetch_tags_for_nodes(&self.pool, &[id.0]).await?;
        node.tags = tag_map.remove(&id.0).unwrap_or_default();
        Ok(node)
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
