use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    edge::{CreateEdgeRequest, Edge, EdgeType, EdgeWithTitles},
    id::{EdgeId, NodeId},
    ontology::EdgeRefusal,
};
use sqlx::PgPool;
use std::collections::HashSet;
use uuid::Uuid;

#[async_trait]
pub trait EdgeRepo: Send + Sync {
    async fn create(&self, req: CreateEdgeRequest) -> Result<Edge, EmberTroveError>;
    async fn get(&self, id: EdgeId) -> Result<Edge, EmberTroveError>;
    async fn delete(&self, id: EdgeId) -> Result<(), EmberTroveError>;
    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Edge>, EmberTroveError>;
    /// Edges touching `node_id`, with both endpoint titles. SECURITY: only
    /// edges whose endpoints are both visible to `subject` (`None` = admin) —
    /// the route checks access to `node_id` alone.
    async fn list_for_node_with_titles(
        &self,
        node_id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<EdgeWithTitles>, EmberTroveError>;
    async fn list_all(&self) -> Result<Vec<Edge>, EmberTroveError>;
    /// Edges visible to `subject`: both endpoints owned by them (`Some(sub)`),
    /// or all edges for an admin (`None`). SECURITY: prevents leaking the node
    /// UUIDs / topology of other owners' private graphs.
    async fn list_visible(&self, subject: Option<&str>) -> Result<Vec<Edge>, EmberTroveError>;
    /// Atomically replace all `wiki_link` edges outgoing from `source_id` with
    /// edges to the given `target_ids`. Self-loop targets are silently skipped.
    async fn sync_wikilinks(
        &self,
        source_id: NodeId,
        target_ids: &[NodeId],
    ) -> Result<(), EmberTroveError>;
}

pub struct PgEdgeRepo {
    pool: PgPool,
}

impl PgEdgeRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct EdgeRow {
    id: Uuid,
    source_id: Uuid,
    target_id: Uuid,
    edge_type: String,
    label: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct EdgeWithTitlesRow {
    id: Uuid,
    source_id: Uuid,
    source_title: String,
    target_id: Uuid,
    target_title: String,
    edge_type: String,
    label: Option<String>,
    created_at: DateTime<Utc>,
}

impl EdgeWithTitlesRow {
    fn into_edge_with_titles(self) -> Result<EdgeWithTitles, EmberTroveError> {
        Ok(EdgeWithTitles {
            id: EdgeId(self.id),
            source_id: NodeId(self.source_id),
            source_title: self.source_title,
            target_id: NodeId(self.target_id),
            target_title: self.target_title,
            edge_type: parse_edge_type(&self.edge_type)?,
            label: self.label,
            created_at: self.created_at,
        })
    }
}

impl EdgeRow {
    fn into_edge(self) -> Result<Edge, EmberTroveError> {
        Ok(Edge {
            id: EdgeId(self.id),
            source_id: NodeId(self.source_id),
            target_id: NodeId(self.target_id),
            edge_type: parse_edge_type(&self.edge_type)?,
            label: self.label,
            created_at: self.created_at,
        })
    }
}

fn parse_edge_type(s: &str) -> Result<EdgeType, EmberTroveError> {
    super::parse_wire(s, "edge_type", EdgeType::parse)
}

/// The rule a new edge of an acyclic type breaks because of the stored
/// graph (#224, §6): a second home for a `contains` child, or a loop. The
/// caller holds the type's advisory lock. The answer names no node; the
/// path may run through nodes the actor can't see (§14).
async fn structural_refusal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &CreateEdgeRequest,
) -> Result<Option<EdgeRefusal>, EmberTroveError> {
    let failed = |e: sqlx::Error| EmberTroveError::Internal(format!("edge check failed: {e}"));
    if req.edge_type == EdgeType::Contains {
        // The same parent again is a duplicate: the unique index answers
        // that with 409 "already linked".
        let has_home: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM edges \
             WHERE edge_type = 'contains' AND target_id = $1 AND source_id <> $2)",
        )
        .bind(req.target_id.0)
        .bind(req.source_id.0)
        .fetch_one(&mut **tx)
        .await
        .map_err(failed)?;
        if has_home {
            return Ok(Some(EdgeRefusal::SecondHome));
        }
    }
    // Would the edge close a loop: is the source reachable from the target?
    // UNION (not UNION ALL) keeps a legacy loop from recursing forever.
    let closes_loop: bool = sqlx::query_scalar(
        r#"
        WITH RECURSIVE reach(id) AS (
            SELECT target_id FROM edges WHERE source_id = $2 AND edge_type = $3::edge_type
            UNION
            SELECT e.target_id FROM edges e JOIN reach r ON e.source_id = r.id
            WHERE e.edge_type = $3::edge_type
        )
        SELECT EXISTS (SELECT 1 FROM reach WHERE id = $1)
        "#,
    )
    .bind(req.source_id.0)
    .bind(req.target_id.0)
    .bind(req.edge_type.as_str())
    .fetch_one(&mut **tx)
    .await
    .map_err(failed)?;
    Ok(closes_loop.then(|| EdgeRefusal::Loop(req.edge_type.clone())))
}

#[async_trait]
impl EdgeRepo for PgEdgeRepo {
    async fn get(&self, id: EdgeId) -> Result<Edge, EmberTroveError> {
        let row = sqlx::query_as::<_, EdgeRow>(
            r#"
            SELECT id, source_id, target_id, edge_type::text, label, created_at
            FROM edges
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("get edge failed: {e}")))?
        .ok_or_else(|| EmberTroveError::NotFound(format!("edge {} not found", id.0)))?;

        row.into_edge()
    }

    async fn create(&self, req: CreateEdgeRequest) -> Result<Edge, EmberTroveError> {
        if req.source_id == req.target_id {
            return Err(EmberTroveError::Validation(
                "self-loops are not allowed".to_string(),
            ));
        }

        let edge_type_str = req.edge_type.as_str();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("create edge failed: {e}")))?;

        if req.edge_type.is_acyclic() {
            // One lock per edge type (#224): without it, two inserts could
            // each pass the checks below and together add a second home or
            // close a loop. Edges of other types don't wait.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ember.edges.' || $1))")
                .bind(edge_type_str)
                .execute(&mut *tx)
                .await
                .map_err(|e| EmberTroveError::Internal(format!("edge lock failed: {e}")))?;
            if let Some(refusal) = structural_refusal(&mut tx, &req).await? {
                return Err(EmberTroveError::Validation(refusal.message()));
            }
        }

        let row = sqlx::query_as::<_, EdgeRow>(
            r#"
            INSERT INTO edges (source_id, target_id, edge_type, label)
            VALUES ($1, $2, $3::edge_type, $4)
            RETURNING id, source_id, target_id, edge_type::text, label, created_at
            "#,
        )
        .bind(req.source_id.0)
        .bind(req.target_id.0)
        .bind(edge_type_str)
        .bind(&req.label)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) => {
                if db_err.constraint() == Some("edges_no_self_loop") {
                    EmberTroveError::Validation("self-loops are not allowed".to_string())
                } else if db_err.constraint() == Some("edges_unique_source_target_type")
                    // `related_to` is unordered (#226): B→A after A→B is
                    // the same edge.
                    || db_err.constraint() == Some("edges_related_to_once")
                {
                    // One edge per (source, target, type) — migration 034 (#129).
                    EmberTroveError::AlreadyExists(format!(
                        "already linked as {}",
                        req.edge_type.label()
                    ))
                } else if db_err.is_foreign_key_violation() {
                    EmberTroveError::NotFound("source or target node not found".to_string())
                } else {
                    EmberTroveError::Internal(format!("create edge failed: {e}"))
                }
            }
            _ => EmberTroveError::Internal(format!("create edge failed: {e}")),
        })?;
        tx.commit()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("create edge failed: {e}")))?;

        row.into_edge()
    }

    async fn delete(&self, id: EdgeId) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM edges WHERE id = $1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete edge failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound(format!("edge {id} not found")));
        }

        Ok(())
    }

    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Edge>, EmberTroveError> {
        let rows = sqlx::query_as::<_, EdgeRow>(
            r#"
            SELECT id, source_id, target_id, edge_type::text, label, created_at
            FROM edges
            WHERE source_id = $1 OR target_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(node_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list edges failed: {e}")))?;

        rows.into_iter().map(EdgeRow::into_edge).collect()
    }

    async fn list_for_node_with_titles(
        &self,
        node_id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<EdgeWithTitles>, EmberTroveError> {
        let rows = sqlx::query_as::<_, EdgeWithTitlesRow>(&format!(
            r#"
            SELECT
                e.id,
                e.source_id,
                sn.title AS source_title,
                e.target_id,
                tn.title AS target_title,
                e.edge_type::text,
                e.label,
                e.created_at
            FROM edges e
            JOIN nodes sn ON sn.id = e.source_id
            JOIN nodes tn ON tn.id = e.target_id
            WHERE (e.source_id = $1 OR e.target_id = $1)
              AND {sn_visible}
              AND {tn_visible}
            ORDER BY e.created_at DESC
            "#,
            sn_visible = super::visible_to("sn", 2),
            tn_visible = super::visible_to("tn", 2),
        ))
        .bind(node_id.0)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list edges with titles failed: {e}")))?;

        rows.into_iter()
            .map(EdgeWithTitlesRow::into_edge_with_titles)
            .collect()
    }

    async fn list_all(&self) -> Result<Vec<Edge>, EmberTroveError> {
        let rows = sqlx::query_as::<_, EdgeRow>(
            r#"
            SELECT id, source_id, target_id, edge_type::text, label, created_at
            FROM edges
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list all edges failed: {e}")))?;

        rows.into_iter().map(EdgeRow::into_edge).collect()
    }

    async fn list_visible(&self, subject: Option<&str>) -> Result<Vec<Edge>, EmberTroveError> {
        let rows = sqlx::query_as::<_, EdgeRow>(
            r#"
            SELECT e.id, e.source_id, e.target_id, e.edge_type::text, e.label, e.created_at
            FROM edges e
            JOIN nodes sn ON sn.id = e.source_id
            JOIN nodes tn ON tn.id = e.target_id
            WHERE ($1::text IS NULL
                   OR (sn.owner_id = $1::text AND tn.owner_id = $1::text))
            ORDER BY e.created_at DESC
            "#,
        )
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list visible edges failed: {e}")))?;

        rows.into_iter().map(EdgeRow::into_edge).collect()
    }

    async fn sync_wikilinks(
        &self,
        source_id: NodeId,
        target_ids: &[NodeId],
    ) -> Result<(), EmberTroveError> {
        // Deduplicate and remove self-loops.
        let targets: HashSet<Uuid> = target_ids
            .iter()
            .map(|id| id.0)
            .filter(|&id| id != source_id.0)
            .collect();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("begin transaction failed: {e}")))?;

        // Remove all existing wiki_link edges from this source node.
        sqlx::query(
            "DELETE FROM edges WHERE source_id = $1 AND edge_type = 'wiki_link'::edge_type",
        )
        .bind(source_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("delete wiki_link edges failed: {e}")))?;

        // Insert one edge per resolved target.
        for target in targets {
            sqlx::query(
                r#"
                INSERT INTO edges (source_id, target_id, edge_type, label)
                VALUES ($1, $2, 'wiki_link'::edge_type, NULL)
                ON CONFLICT (source_id, target_id, edge_type) DO NOTHING
                "#,
            )
            .bind(source_id.0)
            .bind(target)
            .execute(&mut *tx)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("insert wiki_link edge failed: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("commit wiki_link sync failed: {e}")))?;

        Ok(())
    }
}
