//! The data-health report (#208): an account's data that breaks the data
//! model (`docs/ONTOLOGY.md`), computed on read.
//!
//! SECURITY (§14): every method takes the owner as `&str`, never
//! `Option<&str>`. `visible_to` treats a NULL subject as "sees everything"
//! (the admin bypass), and a report must never use it: an admin's report is
//! their own data like anyone's. An item names a node only when the owner
//! may read it, and hidden nodes are never counted.

use async_trait::async_trait;
use std::collections::HashMap;

use common::{
    EmberTroveError,
    data_health::{ITEM_CAP, Ref, RuleCount, RuleId, RuleReport, Violation},
    node::NodeType,
    ontology::{
        ContainmentRule, containment_violation, cyclic_components, cyclic_components_preferring,
    },
};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use super::{parse_wire, visible_to};

#[async_trait]
pub trait DataHealthRepo: Send + Sync {
    /// `owner`'s report for every rule, in [`RuleId::ALL`] order.
    async fn report_for(&self, owner: &str) -> Result<Vec<RuleReport>, EmberTroveError>;

    /// Every rule's count across the installation, for the admin view: one
    /// evaluation over all data (not a sum of per-account reports, so a
    /// cross-owner loop or pair counts once). Counts only: no titles, ids
    /// or content ever leave this method (§14).
    async fn counts_all(&self) -> Result<Vec<RuleCount>, EmberTroveError>;
}

pub struct PgDataHealthRepo {
    pool: PgPool,
}

impl PgDataHealthRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db(e: sqlx::Error) -> EmberTroveError {
    EmberTroveError::Internal(format!("data health query failed: {e}"))
}

/// `LIMIT` for a rule's items: the cap, as the SQL integer it binds as.
const LIMIT: i64 = ITEM_CAP as i64;

#[async_trait]
impl DataHealthRepo for PgDataHealthRepo {
    async fn report_for(&self, owner: &str) -> Result<Vec<RuleReport>, EmberTroveError> {
        // One snapshot for every rule, so the counts agree with each other.
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;

        // Each edge type is fetched once (both ends visible) and feeds every
        // rule on it; which rows a rule may use is filtered in Rust.
        let contains = edges_in_view(&mut tx, owner, "contains").await?;
        let depends_on = edges_in_view(&mut tx, owner, "depends_on").await?;
        let derived_from = edges_in_view(&mut tx, owner, "derived_from").await?;
        let related_to = edges_in_view(&mut tx, owner, "related_to").await?;
        let [contained_reference, not_allowed] = containment(&contains)?;
        let reports = vec![
            tasks_on_non_projects(&mut tx, owner).await?,
            orphan_tasks(&mut tx, owner).await?,
            memberships_on_non_projects(&mut tx, owner).await?,
            statuses_on_non_projects(&mut tx, owner).await?,
            multiple_homes(&contains),
            contained_reference,
            not_allowed,
            depends_on_areas(&depends_on)?,
            related_both_ways(&related_to),
            loops(&contains, RuleId::ContainsCycle),
            loops(&depends_on, RuleId::DependsOnCycle),
            loops(&derived_from, RuleId::DerivedFromCycle),
        ];
        tx.commit().await.map_err(db)?;

        // Built in `RuleId::ALL` order.
        debug_assert!(reports.iter().zip(RuleId::ALL).all(|(r, id)| r.rule == *id));
        Ok(reports)
    }

    async fn counts_all(&self) -> Result<Vec<RuleCount>, EmberTroveError> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let counts = all_counts(&mut tx).await?;
        tx.commit().await.map_err(db)?;
        Ok(counts)
    }
}

// ── Rules on what a node kind may carry (§3.2, §7.2, §9.3) ───────────────────

#[derive(FromRow)]
struct TaskRow {
    id: Uuid,
    title: String,
    node_id: Option<Uuid>,
    node_title: Option<String>,
    node_archived: Option<bool>,
    closed: bool,
    total: i64,
}

/// The exact count from a rule's `count(*) OVER ()` column (the same on
/// every row; no rows means none).
fn count_of(total: Option<i64>) -> u64 {
    total.and_then(|n| u64::try_from(n).ok()).unwrap_or(0)
}

/// Tasks on my nodes that aren't Projects. Tasks are listed for the node's
/// owner whoever created them: they're the owner's to move.
async fn tasks_on_non_projects(
    conn: &mut PgConnection,
    owner: &str,
) -> Result<RuleReport, EmberTroveError> {
    let rows: Vec<TaskRow> = sqlx::query_as(
        r#"
        SELECT t.id, t.title, n.id AS node_id, n.title AS node_title,
               n.status = 'archived' AS node_archived,
               t.status IN ('done', 'cancelled') AS closed, count(*) OVER () AS total
        FROM node_tasks t
        JOIN nodes n ON n.id = t.node_id
        WHERE n.owner_id = $1
          AND n.node_type <> 'project'
          AND t.deleted_at IS NULL
        ORDER BY n.title, t.created_at, t.id
        LIMIT $2
        "#,
    )
    .bind(owner)
    .bind(LIMIT)
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    let count = count_of(rows.first().map(|r| r.total));
    let items = rows.into_iter().map(task_violation).collect();
    Ok(RuleReport::with_count(
        RuleId::TaskOnNonProject,
        count,
        items,
    ))
}

fn task_violation(r: TaskRow) -> Violation {
    let archived = r.node_archived.unwrap_or(false);
    let related = match (r.node_id, r.node_title) {
        (Some(id), Some(title)) => vec![Ref::node(id, title, archived)],
        _ => vec![],
    };
    Violation::new(
        Ref::task(r.id, r.title, r.node_id, archived).with_closed(r.closed),
        related,
    )
}

/// My tasks that belong to no node at all (today's Inbox), any status.
async fn orphan_tasks(conn: &mut PgConnection, owner: &str) -> Result<RuleReport, EmberTroveError> {
    let rows: Vec<TaskRow> = sqlx::query_as(
        r#"
        SELECT t.id, t.title, NULL::uuid AS node_id, NULL::text AS node_title,
               NULL::bool AS node_archived,
               t.status IN ('done', 'cancelled') AS closed, count(*) OVER () AS total
        FROM node_tasks t
        WHERE t.owner_id = $1
          AND t.node_id IS NULL
          AND t.deleted_at IS NULL
        -- Newest first: a long list shows what was just captured.
        ORDER BY t.created_at DESC, t.id
        LIMIT $2
        "#,
    )
    .bind(owner)
    .bind(LIMIT)
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    let count = count_of(rows.first().map(|r| r.total));
    let items = rows.into_iter().map(task_violation).collect();
    Ok(RuleReport::with_count(RuleId::OrphanTask, count, items))
}

#[derive(FromRow)]
struct MembershipRow {
    membership_id: Uuid,
    node_owned: bool,
    person_id: Uuid,
    person_name: String,
    node_id: Uuid,
    node_title: String,
    node_archived: bool,
    total: i64,
}

/// People of mine placed on a node that isn't a Project. People are only
/// ever named to their owner (§14).
async fn memberships_on_non_projects(
    conn: &mut PgConnection,
    owner: &str,
) -> Result<RuleReport, EmberTroveError> {
    let sql = format!(
        r#"
        SELECT m.id AS membership_id, n.owner_id = $1 AS node_owned,
               p.id AS person_id, p.display_name AS person_name,
               n.id AS node_id, n.title AS node_title,
               n.status = 'archived' AS node_archived, count(*) OVER () AS total
        FROM node_people m
        JOIN people p ON p.id = m.person_id
        JOIN nodes n ON n.id = m.node_id
        WHERE p.owner_id = $1
          AND n.node_type <> 'project'
          AND {visible}
        ORDER BY p.display_name, n.title, m.id
        LIMIT $2
        "#,
        visible = visible_to("n", 1),
    );
    let rows: Vec<MembershipRow> = sqlx::query_as(&sql)
        .bind(owner)
        .bind(LIMIT)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)?;
    let count = count_of(rows.first().map(|r| r.total));
    let items = rows
        .into_iter()
        .map(|r| {
            Violation::new(
                Ref::person(r.person_id, r.person_name),
                vec![Ref::node(r.node_id, r.node_title, r.node_archived)],
            )
            .with_membership(r.membership_id, r.node_owned)
        })
        .collect();
    Ok(RuleReport::with_count(
        RuleId::MembershipOnNonProject,
        count,
        items,
    ))
}

#[derive(FromRow)]
struct NodeRow {
    id: Uuid,
    title: String,
    archived: bool,
    total: i64,
}

/// My nodes that aren't Projects but carry a project status.
async fn statuses_on_non_projects(
    conn: &mut PgConnection,
    owner: &str,
) -> Result<RuleReport, EmberTroveError> {
    let rows: Vec<NodeRow> = sqlx::query_as(
        r#"
        SELECT id, title, status = 'archived' AS archived, count(*) OVER () AS total
        FROM nodes
        WHERE owner_id = $1
          AND node_type <> 'project'
          AND project_status IS NOT NULL
        ORDER BY title, id
        LIMIT $2
        "#,
    )
    .bind(owner)
    .bind(LIMIT)
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    let count = count_of(rows.first().map(|r| r.total));
    let items = rows
        .into_iter()
        .map(|r| Violation::new(Ref::node(r.id, r.title, r.archived), vec![]))
        .collect();
    Ok(RuleReport::with_count(
        RuleId::StatusOnNonProject,
        count,
        items,
    ))
}

// ── Rules on edges (§3.3, §6) ─────────────────────────────────────────────────

/// One edge whose two ends the caller can see, with what they may know of
/// each end.
#[derive(FromRow)]
struct EdgeRow {
    source_id: Uuid,
    source_title: String,
    source_type: String,
    source_archived: bool,
    source_owned: bool,
    target_id: Uuid,
    target_title: String,
    target_type: String,
    target_archived: bool,
    target_owned: bool,
}

impl EdgeRow {
    fn source(&self) -> Ref {
        Ref::node(
            self.source_id,
            self.source_title.clone(),
            self.source_archived,
        )
    }
    fn target(&self) -> Ref {
        Ref::node(
            self.target_id,
            self.target_title.clone(),
            self.target_archived,
        )
    }
    /// The caller owns an end: the edge is theirs to report and fix.
    fn has_stake(&self) -> bool {
        self.source_owned || self.target_owned
    }
}

/// Every edge of `edge_type` whose two ends the caller can see, ordered by
/// target then source title. The one query behind every edge rule, so
/// visibility is decided in exactly one place (§14): an edge touching a
/// node the caller can't see is never loaded, so it can't be named,
/// counted, or close a loop.
async fn edges_in_view(
    conn: &mut PgConnection,
    owner: &str,
    edge_type: &str,
) -> Result<Vec<EdgeRow>, EmberTroveError> {
    let sql = format!(
        r#"
        SELECT s.id AS source_id, s.title AS source_title,
               s.node_type::text AS source_type, s.status = 'archived' AS source_archived,
               s.owner_id = $1 AS source_owned,
               t.id AS target_id, t.title AS target_title,
               t.node_type::text AS target_type, t.status = 'archived' AS target_archived,
               t.owner_id = $1 AS target_owned
        FROM edges e
        JOIN nodes s ON s.id = e.source_id
        JOIN nodes t ON t.id = e.target_id
        WHERE e.edge_type = $2::edge_type
          AND {s_visible}
          AND {t_visible}
        ORDER BY t.title, s.title, e.id
        "#,
        s_visible = visible_to("s", 1),
        t_visible = visible_to("t", 1),
    );
    sqlx::query_as(&sql)
        .bind(owner)
        .bind(edge_type)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)
}

fn node_type(s: &str) -> Result<NodeType, EmberTroveError> {
    parse_wire(s, "node type", NodeType::parse)
}

/// Nodes of mine with more than one home I can see. Counted over visible
/// parents only, so a hidden parent can neither be named nor hide two
/// visible ones. Only my own children: a node of someone else's with two
/// homes is theirs to fix, even when one home is mine.
fn multiple_homes(contains: &[EdgeRow]) -> RuleReport {
    // Rows come ordered by child title, then parent title: group runs.
    let mut items: Vec<Violation> = Vec::new();
    let mut current: Option<(Ref, Vec<Ref>)> = None;
    for r in contains.iter().filter(|r| r.target_owned) {
        match &mut current {
            Some((child, parents)) if child.id == r.target_id => parents.push(r.source()),
            _ => {
                if let Some((child, parents)) = current.take() {
                    push_if_several(&mut items, child, parents);
                }
                current = Some((r.target(), vec![r.source()]));
            }
        }
    }
    if let Some((child, parents)) = current {
        push_if_several(&mut items, child, parents);
    }
    RuleReport::new(RuleId::MultipleHomes, items)
}

fn push_if_several(items: &mut Vec<Violation>, child: Ref, parents: Vec<Ref>) {
    if parents.len() > 1 {
        items.push(Violation::new(child, parents));
    }
}

/// `contains` edges of mine that break the containment table, classified by
/// the same function the write gates will use
/// (`ontology::containment_violation`), so the report and the enforcement
/// can't disagree. Each bad edge lands under exactly one rule.
fn containment(contains: &[EdgeRow]) -> Result<[RuleReport; 2], EmberTroveError> {
    let mut contained_reference = Vec::new();
    let mut not_allowed = Vec::new();
    for r in contains.iter().filter(|r| r.has_stake()) {
        let parent = node_type(&r.source_type)?;
        let child = node_type(&r.target_type)?;
        let bucket = match containment_violation(&parent, &child) {
            Some(ContainmentRule::ContainedReference) => &mut contained_reference,
            Some(ContainmentRule::NotAllowed) => &mut not_allowed,
            None => continue,
        };
        bucket.push(Violation::new(r.target(), vec![r.source()]));
    }
    Ok([
        RuleReport::new(RuleId::ContainedReference, contained_reference),
        RuleReport::new(RuleId::ContainmentNotAllowed, not_allowed),
    ])
}

/// `depends_on` edges of mine whose target is an Area (§6.4).
fn depends_on_areas(depends_on: &[EdgeRow]) -> Result<RuleReport, EmberTroveError> {
    let mut items = Vec::new();
    for r in depends_on.iter().filter(|r| r.has_stake()) {
        if node_type(&r.target_type)? == NodeType::Area {
            items.push(Violation::new(r.source(), vec![r.target()]));
        }
    }
    Ok(RuleReport::new(RuleId::DependsOnArea, items))
}

/// Pairs of mine linked by `related_to` in both directions (§6.2), each
/// pair once: from the edge whose source sorts first.
fn related_both_ways(related_to: &[EdgeRow]) -> RuleReport {
    let present: std::collections::HashSet<(Uuid, Uuid)> = related_to
        .iter()
        .map(|r| (r.source_id, r.target_id))
        .collect();
    let items = related_to
        .iter()
        .filter(|r| r.has_stake() && r.source_id < r.target_id)
        .filter(|r| present.contains(&(r.target_id, r.source_id)))
        .map(|r| Violation::new(r.source(), vec![r.target()]))
        .collect();
    RuleReport::new(RuleId::RelatedToBothWays, items)
}

// ── Loops (§6.1, §6.4, §6.5) ──────────────────────────────────────────────────

/// Groups of nodes that loop back on themselves through `edges` and include
/// a node of mine. Every visible edge is walked, so a loop through someone
/// else's shared nodes is still found. The path shown runs through my own
/// node (the smallest one in the group), so it's a loop I can break.
/// Counted per group: two loops through one shared node are one group.
fn loops(edges: &[EdgeRow], rule: RuleId) -> RuleReport {
    let mut nodes: HashMap<Uuid, (Ref, bool)> = HashMap::new();
    let mut pairs = Vec::with_capacity(edges.len());
    for r in edges {
        pairs.push((r.source_id, r.target_id));
        nodes
            .entry(r.source_id)
            .or_insert_with(|| (r.source(), r.source_owned));
        nodes
            .entry(r.target_id)
            .or_insert_with(|| (r.target(), r.target_owned));
    }
    let owned = |id: &Uuid| nodes.get(id).is_some_and(|(_, owned)| *owned);
    let refs = |ids: &[Uuid]| -> Vec<Ref> {
        ids.iter()
            .filter_map(|id| nodes.get(id).map(|(r, _)| r.clone()))
            .collect()
    };
    let items = cyclic_components_preferring(&pairs, owned)
        .into_iter()
        .filter(|c| c.members.iter().any(owned))
        .filter_map(|c| {
            // The loop as a path: its first node (mine), then the rest.
            let (first, rest) = c.cycle.split_first()?;
            let subject = refs(std::slice::from_ref(first)).pop()?;
            Some(Violation::new(subject, refs(rest)))
        })
        .collect();
    RuleReport::new(rule, items)
}

// ── Admin counts (§14: counts only) ───────────────────────────────────────────

#[derive(FromRow)]
struct SqlCounts {
    task_on_non_project: i64,
    orphan_task: i64,
    membership_on_non_project: i64,
    status_on_non_project: i64,
    multiple_homes: i64,
    depends_on_area: i64,
    related_to_both_ways: i64,
}

/// Every rule over all data, no subject filter. Only numbers are read
/// back: titles never enter this path.
///
/// This is the installation-wide truth that gates each rule's database
/// constraint, so it can exceed the sum of the accounts' reports: a second
/// home in a node its child's owner can't see counts here but shows in no
/// one's report. That gap is what the recorded fallback policy is for.
async fn all_counts(conn: &mut PgConnection) -> Result<Vec<RuleCount>, EmberTroveError> {
    let sql: SqlCounts = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM node_tasks t JOIN nodes n ON n.id = t.node_id
            WHERE n.node_type <> 'project' AND t.deleted_at IS NULL) AS task_on_non_project,
          (SELECT count(*) FROM node_tasks
            WHERE node_id IS NULL AND deleted_at IS NULL) AS orphan_task,
          (SELECT count(*) FROM node_people m JOIN nodes n ON n.id = m.node_id
            WHERE n.node_type <> 'project') AS membership_on_non_project,
          (SELECT count(*) FROM nodes
            WHERE node_type <> 'project' AND project_status IS NOT NULL) AS status_on_non_project,
          (SELECT count(*) FROM (
             SELECT target_id FROM edges WHERE edge_type = 'contains'
             GROUP BY target_id HAVING count(*) > 1) homes) AS multiple_homes,
          (SELECT count(*) FROM edges e JOIN nodes t ON t.id = e.target_id
            WHERE e.edge_type = 'depends_on' AND t.node_type = 'area') AS depends_on_area,
          (SELECT count(*) FROM edges e
            WHERE e.edge_type = 'related_to' AND e.source_id < e.target_id
              AND EXISTS (SELECT 1 FROM edges r WHERE r.edge_type = 'related_to'
                          AND r.source_id = e.target_id AND r.target_id = e.source_id))
            AS related_to_both_ways
        "#,
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(db)?;

    // Containment per kind pair (at most 25 rows), classified by the same
    // function as the report.
    let pairs: Vec<(String, String, i64)> = sqlx::query_as(
        r#"
        SELECT s.node_type::text, t.node_type::text, count(*)
        FROM edges e
        JOIN nodes s ON s.id = e.source_id
        JOIN nodes t ON t.id = e.target_id
        WHERE e.edge_type = 'contains'
        GROUP BY 1, 2
        "#,
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    let (mut contained_reference, mut not_allowed) = (0u64, 0u64);
    for (parent, child, n) in &pairs {
        let n = u64::try_from(*n).unwrap_or(0);
        match containment_violation(&node_type(parent)?, &node_type(child)?) {
            Some(ContainmentRule::ContainedReference) => contained_reference += n,
            Some(ContainmentRule::NotAllowed) => not_allowed += n,
            None => {}
        }
    }

    let n = |c: i64| u64::try_from(c).unwrap_or(0);
    let counts = vec![
        RuleCount {
            rule: RuleId::TaskOnNonProject,
            count: n(sql.task_on_non_project),
        },
        RuleCount {
            rule: RuleId::OrphanTask,
            count: n(sql.orphan_task),
        },
        RuleCount {
            rule: RuleId::MembershipOnNonProject,
            count: n(sql.membership_on_non_project),
        },
        RuleCount {
            rule: RuleId::StatusOnNonProject,
            count: n(sql.status_on_non_project),
        },
        RuleCount {
            rule: RuleId::MultipleHomes,
            count: n(sql.multiple_homes),
        },
        RuleCount {
            rule: RuleId::ContainedReference,
            count: contained_reference,
        },
        RuleCount {
            rule: RuleId::ContainmentNotAllowed,
            count: not_allowed,
        },
        RuleCount {
            rule: RuleId::DependsOnArea,
            count: n(sql.depends_on_area),
        },
        RuleCount {
            rule: RuleId::RelatedToBothWays,
            count: n(sql.related_to_both_ways),
        },
        RuleCount {
            rule: RuleId::ContainsCycle,
            count: loop_count(conn, "contains").await?,
        },
        RuleCount {
            rule: RuleId::DependsOnCycle,
            count: loop_count(conn, "depends_on").await?,
        },
        RuleCount {
            rule: RuleId::DerivedFromCycle,
            count: loop_count(conn, "derived_from").await?,
        },
    ];
    debug_assert!(counts.iter().zip(RuleId::ALL).all(|(c, id)| c.rule == *id));
    Ok(counts)
}

/// How many looping groups `edge_type` edges form across all data.
async fn loop_count(conn: &mut PgConnection, edge_type: &str) -> Result<u64, EmberTroveError> {
    let pairs: Vec<(Uuid, Uuid)> =
        sqlx::query_as("SELECT source_id, target_id FROM edges WHERE edge_type = $1::edge_type")
            .bind(edge_type)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
    Ok(cyclic_components(&pairs).len() as u64)
}
