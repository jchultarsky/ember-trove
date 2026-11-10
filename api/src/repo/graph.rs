use std::collections::HashMap;

use async_trait::async_trait;
use common::{
    EmberTroveError,
    graph::{GRAPH_PREVIEW_CHARS, GraphNode, GraphTag, NodePosition},
    id::{NodeId, TagId},
    markdown::plain_preview,
    node::{NodeStatus, NodeType},
};
use sqlx::PgPool;
use uuid::Uuid;

/// How much of a body the graph reads to make its preview. The preview
/// keeps 100 characters of text; this leaves room for headings and code
/// the preview skips, without reading whole bodies (P1, 2026-11-02).
const PREVIEW_SOURCE_CHARS: i32 = 2_000;

#[async_trait]
pub trait GraphRepo: Send + Sync {
    /// Every node the graph draws, without bodies (a short plain-text
    /// preview instead). SECURITY: the same visibility as `GET /nodes`:
    /// `Some(sub)` gets the nodes it owns or holds a permission on, `None`
    /// (admin) all of them. Archived nodes are included, as the graph always
    /// showed them.
    async fn list_nodes(&self, subject: Option<&str>) -> Result<Vec<GraphNode>, EmberTroveError>;
    /// List node positions. SECURITY: scoped to `subject`'s owned nodes
    /// (`Some(sub)`); `None` (admin) returns all. Prevents enumerating other
    /// owners' node IDs/coordinates.
    async fn list_positions(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<NodePosition>, EmberTroveError>;
    /// Batch upsert positions. SECURITY: only positions for nodes owned by
    /// `subject` are written (`None` = admin, no filter); rows for nodes the
    /// caller doesn't own are silently dropped, preventing cross-user tampering.
    async fn save_positions(
        &self,
        positions: &[(Uuid, f64, f64)],
        subject: Option<&str>,
    ) -> Result<(), EmberTroveError>;
}

pub struct PgGraphRepo {
    pool: PgPool,
}

impl PgGraphRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct GraphNodeRow {
    id: Uuid,
    title: String,
    node_type: String,
    status: String,
    pinned: bool,
    body_head: Option<String>,
}

#[derive(sqlx::FromRow)]
struct GraphTagRow {
    node_id: Uuid,
    id: Uuid,
    color: String,
}

#[derive(sqlx::FromRow)]
struct NodePositionRow {
    node_id: Uuid,
    x: f64,
    y: f64,
}

/// Saved graph positions: one owner's, or everyone's with `None`. A free
/// function so the backup can run it on its one snapshot transaction
/// (`backup::read_snapshot`).
pub(crate) async fn list_positions_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    subject: Option<&str>,
) -> Result<Vec<NodePosition>, EmberTroveError> {
    let rows = sqlx::query_as::<_, NodePositionRow>(
        "SELECT p.node_id, p.x, p.y
         FROM node_positions p
         JOIN nodes n ON n.id = p.node_id
         WHERE ($1::text IS NULL OR n.owner_id = $1::text)",
    )
    .bind(subject)
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list_positions failed: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|r| NodePosition {
            node_id: NodeId(r.node_id),
            x: r.x,
            y: r.y,
        })
        .collect())
}

#[async_trait]
impl GraphRepo for PgGraphRepo {
    async fn list_nodes(&self, subject: Option<&str>) -> Result<Vec<GraphNode>, EmberTroveError> {
        let sql = format!(
            "SELECT n.id, n.title, n.node_type::text AS node_type, n.status::text AS status,
                    n.pinned, substring(n.body FROM 1 FOR $2) AS body_head
             FROM nodes n
             WHERE {visible}
             ORDER BY n.pinned DESC, n.updated_at DESC",
            visible = super::visible_among_many("n", 1),
        );
        let rows = sqlx::query_as::<_, GraphNodeRow>(&sql)
            .bind(subject)
            .bind(PREVIEW_SOURCE_CHARS)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("graph nodes failed: {e}")))?;

        let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
        let mut tags: HashMap<Uuid, Vec<GraphTag>> = HashMap::new();
        if !ids.is_empty() {
            let tag_rows = sqlx::query_as::<_, GraphTagRow>(
                "SELECT nt.node_id, t.id, t.color
                 FROM tags t JOIN node_tags nt ON nt.tag_id = t.id
                 WHERE nt.node_id = ANY($1)
                 ORDER BY t.name",
            )
            .bind(&ids)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("graph tags failed: {e}")))?;
            for t in tag_rows {
                tags.entry(t.node_id).or_default().push(GraphTag {
                    id: TagId(t.id),
                    color: t.color,
                });
            }
        }

        rows.into_iter()
            .map(|r| {
                Ok(GraphNode {
                    id: NodeId(r.id),
                    title: r.title,
                    node_type: super::parse_wire(&r.node_type, "node_type", NodeType::parse)?,
                    status: super::parse_wire(&r.status, "node_status", NodeStatus::parse)?,
                    pinned: r.pinned,
                    tags: tags.remove(&r.id).unwrap_or_default(),
                    preview: r
                        .body_head
                        .as_deref()
                        .and_then(|b| plain_preview(b, GRAPH_PREVIEW_CHARS)),
                })
            })
            .collect()
    }

    async fn list_positions(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<NodePosition>, EmberTroveError> {
        list_positions_on(&self.pool, subject).await
    }

    async fn save_positions(
        &self,
        positions: &[(Uuid, f64, f64)],
        subject: Option<&str>,
    ) -> Result<(), EmberTroveError> {
        if positions.is_empty() {
            return Ok(());
        }

        // Single round-trip batched UPSERT using parallel-array UNNEST.
        // Earlier impl issued one INSERT per row inside a transaction (N
        // round-trips to Postgres) plus a `DELETE FROM node_positions` that
        // wiped all users' rows; the FK cascade on `nodes(id)` already
        // cleans up positions when a node is deleted, so the delete is
        // redundant.
        let (ids, xs, ys): (Vec<Uuid>, Vec<f64>, Vec<f64>) = positions.iter().fold(
            (
                Vec::with_capacity(positions.len()),
                Vec::with_capacity(positions.len()),
                Vec::with_capacity(positions.len()),
            ),
            |(mut ids, mut xs, mut ys), (id, x, y)| {
                ids.push(*id);
                xs.push(*x);
                ys.push(*y);
                (ids, xs, ys)
            },
        );

        // SECURITY: join to `nodes` and filter by owner so a caller can only
        // write positions for nodes they own (admins pass NULL = no filter).
        // Rows referencing other owners' (or nonexistent) nodes are dropped.
        sqlx::query(
            "INSERT INTO node_positions (node_id, x, y)
             SELECT u.node_id, u.x, u.y
             FROM UNNEST($1::uuid[], $2::double precision[], $3::double precision[])
                  AS u(node_id, x, y)
             JOIN nodes n ON n.id = u.node_id
             WHERE ($4::text IS NULL OR n.owner_id = $4::text)
             ON CONFLICT (node_id) DO UPDATE
             SET x = EXCLUDED.x, y = EXCLUDED.y, updated_at = now()",
        )
        .bind(&ids[..])
        .bind(&xs[..])
        .bind(&ys[..])
        .bind(subject)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("save_positions failed: {e}")))?;

        Ok(())
    }
}
