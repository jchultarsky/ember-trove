//! The data-health report (#208): an account's data that breaks the data
//! model (`docs/ONTOLOGY.md`), computed on read.
//!
//! SECURITY (§14): every method takes the owner as `&str`, never
//! `Option<&str>`. `visible_to` treats a NULL subject as "sees everything"
//! (the admin bypass), and a report must never use it: an admin's report is
//! their own data like anyone's. An item names a node only when the owner
//! may read it, and hidden nodes are never counted.

use async_trait::async_trait;
use common::{
    EmberTroveError,
    data_health::{ITEM_CAP, Ref, RuleId, RuleReport, Violation},
    node::NodeType,
    ontology::{ContainmentRule, containment_violation},
};
use sqlx::{FromRow, PgConnection, PgPool};
use uuid::Uuid;

use super::{parse_wire, visible_to};

#[async_trait]
pub trait DataHealthRepo: Send + Sync {
    /// `owner`'s report for every rule checked in SQL (all but the loops),
    /// in [`RuleId::ALL`] order.
    async fn report_for(&self, owner: &str) -> Result<Vec<RuleReport>, EmberTroveError>;
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

        // `contains` edges feed two rules; fetch them once.
        let contains = edges_in_view(&mut tx, owner, "contains", "").await?;
        let [contained_reference, not_allowed] = containment(&contains)?;
        let reports = vec![
            tasks_on_non_projects(&mut tx, owner).await?,
            orphan_tasks(&mut tx, owner).await?,
            memberships_on_non_projects(&mut tx, owner).await?,
            statuses_on_non_projects(&mut tx, owner).await?,
            multiple_homes(&contains),
            contained_reference,
            not_allowed,
            depends_on_areas(&mut tx, owner).await?,
            related_both_ways(&mut tx, owner).await?,
        ];
        tx.commit().await.map_err(db)?;

        // Built in `RuleId::ALL` order (the loops follow in S1b-3).
        debug_assert!(reports.iter().zip(RuleId::ALL).all(|(r, id)| r.rule == *id));
        Ok(reports)
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
               n.status = 'archived' AS node_archived, count(*) OVER () AS total
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
    Violation::new(Ref::task(r.id, r.title, r.node_id, archived), related)
}

/// My tasks that belong to no node at all (today's Inbox), any status.
async fn orphan_tasks(conn: &mut PgConnection, owner: &str) -> Result<RuleReport, EmberTroveError> {
    let rows: Vec<TaskRow> = sqlx::query_as(
        r#"
        SELECT t.id, t.title, NULL::uuid AS node_id, NULL::text AS node_title,
               NULL::bool AS node_archived, count(*) OVER () AS total
        FROM node_tasks t
        WHERE t.owner_id = $1
          AND t.node_id IS NULL
          AND t.deleted_at IS NULL
        ORDER BY t.created_at, t.id
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
        SELECT p.id AS person_id, p.display_name AS person_name,
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

/// One edge with both ends, as the rules below need them.
#[derive(FromRow)]
struct EdgeRow {
    source_id: Uuid,
    source_title: String,
    source_type: String,
    source_archived: bool,
    target_id: Uuid,
    target_title: String,
    target_type: String,
    target_archived: bool,
    /// The caller owns the target (the child, for `contains`).
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
}

/// Edges of `edge_type` that `owner` has a stake in (owns either end) and
/// can fully see (both ends visible): only those may be named to them.
async fn edges_in_view(
    conn: &mut PgConnection,
    owner: &str,
    edge_type: &str,
    extra_where: &str,
) -> Result<Vec<EdgeRow>, EmberTroveError> {
    // SECURITY: `extra_where` is a compile-time literal from this module,
    // never user input; every value is a bind parameter.
    let sql = format!(
        r#"
        SELECT s.id AS source_id, s.title AS source_title,
               s.node_type::text AS source_type, s.status = 'archived' AS source_archived,
               t.id AS target_id, t.title AS target_title,
               t.node_type::text AS target_type, t.status = 'archived' AS target_archived,
               t.owner_id = $1 AS target_owned
        FROM edges e
        JOIN nodes s ON s.id = e.source_id
        JOIN nodes t ON t.id = e.target_id
        WHERE e.edge_type = $2::edge_type
          AND (s.owner_id = $1 OR t.owner_id = $1)
          AND {s_visible}
          AND {t_visible}
          {extra_where}
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
/// parents only (`edges_in_view`), so a hidden parent can neither be named
/// nor hide two visible ones. Only my own children: a node of someone
/// else's with two homes is theirs to fix, even when one home is mine.
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

/// `contains` edges that break the containment table, classified by the
/// same function the write gates will use (`ontology::containment_violation`),
/// so the report and the enforcement can't disagree. Each bad edge lands
/// under exactly one rule.
fn containment(contains: &[EdgeRow]) -> Result<[RuleReport; 2], EmberTroveError> {
    let mut contained_reference = Vec::new();
    let mut not_allowed = Vec::new();
    for r in contains {
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

/// `depends_on` edges whose target is an Area (§6.4).
async fn depends_on_areas(
    conn: &mut PgConnection,
    owner: &str,
) -> Result<RuleReport, EmberTroveError> {
    let rows = edges_in_view(conn, owner, "depends_on", "AND t.node_type = 'area'").await?;
    let items = rows
        .iter()
        .map(|r| Violation::new(r.source(), vec![r.target()]))
        .collect();
    Ok(RuleReport::new(RuleId::DependsOnArea, items))
}

/// Pairs linked by `related_to` in both directions (§6.2), each pair once:
/// from the edge whose source sorts first.
async fn related_both_ways(
    conn: &mut PgConnection,
    owner: &str,
) -> Result<RuleReport, EmberTroveError> {
    let rows = edges_in_view(
        conn,
        owner,
        "related_to",
        "AND e.source_id < e.target_id \
         AND EXISTS (SELECT 1 FROM edges r WHERE r.edge_type = 'related_to' \
                     AND r.source_id = e.target_id AND r.target_id = e.source_id)",
    )
    .await?;
    let items = rows
        .iter()
        .map(|r| Violation::new(r.source(), vec![r.target()]))
        .collect();
    Ok(RuleReport::new(RuleId::RelatedToBothWays, items))
}
