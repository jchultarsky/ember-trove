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
    /// Replace the `from` link `id` with a `to` link between the same two
    /// nodes (#228), keeping its label and age, in one transaction. With
    /// `swap` the new link runs target → source (a symmetric type, from
    /// the end the caller edits). An identical link already there absorbs
    /// it. The caller has checked rights and `to` (`convert_ends`).
    async fn convert(
        &self,
        id: EdgeId,
        from: EdgeType,
        to: EdgeType,
        swap: bool,
    ) -> Result<(), EmberTroveError>;
    async fn list_for_node(&self, node_id: NodeId) -> Result<Vec<Edge>, EmberTroveError>;
    /// The node's one home (#228): its `contains` link, if any. Whatever
    /// the caller may see; the route decides what to tell them.
    async fn home_of(&self, child: NodeId) -> Result<Option<Edge>, EmberTroveError>;
    /// Make `parent` the node's one home, or let it stand alone (`None`),
    /// under the `contains` lock (#228). Every other home goes, including
    /// one the caller can't see (§6.1: the child's editor may free it).
    /// A loop is refused (422). The route has checked the rights and kinds.
    async fn set_home(
        &self,
        child: NodeId,
        parent: Option<NodeId>,
    ) -> Result<Option<Edge>, EmberTroveError>;
    /// Edges touching `node_id`, with both endpoint titles, each end's
    /// kind and standing, and whether `subject` may remove the link (S7a).
    /// SECURITY: only edges whose endpoints are both visible to `subject`
    /// (`None` = admin) — the route checks access to `node_id` alone. So
    /// every list, count and notice the page builds from this answer is
    /// cut to what its reader can see (§14).
    async fn list_for_node_with_titles(
        &self,
        node_id: NodeId,
        subject: Option<&str>,
    ) -> Result<Vec<EdgeWithTitles>, EmberTroveError>;
    /// What `node`'s unmet dependencies wait for in turn (S7b, §6.4):
    /// the chain beyond its own "Depends on" list.
    ///
    /// SECURITY: every link of the walk is one `subject` (`None` =
    /// admin) would find in that hop's own
    /// `list_for_node_with_titles`: both ends visible to them. The walk
    /// stops at a node they can't see and shows nothing beyond it, so
    /// "stops here" and "nothing further" are one answer (§14). The
    /// route checks access to `node` alone.
    async fn blockers(
        &self,
        node: NodeId,
        subject: Option<&str>,
    ) -> Result<common::blockers::NodeBlockers, EmberTroveError>;
    /// How many links of `node` are its editors' to clear and are not
    /// shown (#349, ONTOLOGY §14): of a type and direction
    /// `EdgeType::is_its_nodes_to_clear` accepts, with the other end
    /// visible **neither to `viewer` nor to the node's owner**.
    ///
    /// SECURITY: the owner half is what keeps this off the owner's own
    /// private links: someone the node is shared with as an editor is not
    /// told of them and cannot clear them. `None` (an admin) is shown
    /// every link, so the count is 0. The caller has checked that
    /// `viewer` may **edit** `node`, and the statement holds to it: a
    /// viewer is told nothing (0).
    async fn not_shown(&self, node: NodeId, viewer: Option<&str>) -> Result<u32, EmberTroveError>;
    /// Remove those links, and say how many went. One statement that
    /// applies the rule itself: never ids taken from a count, so what
    /// became visible meanwhile stays. The caller has checked that
    /// `viewer` may edit `node`.
    async fn remove_not_shown(
        &self,
        node: NodeId,
        viewer: Option<&str>,
    ) -> Result<u64, EmberTroveError>;
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
    source_type: String,
    source_status: String,
    source_state: Option<String>,
    target_type: String,
    target_status: String,
    target_state: Option<String>,
    edits_source: bool,
    edits_target: bool,
}

/// One end of a link as its list shows it (S7a): kind and standing.
fn edge_end(
    node_type: &str,
    status: &str,
    state: Option<&str>,
) -> Result<common::edge::EdgeEnd, EmberTroveError> {
    use common::{
        node::{NodeStatus, NodeType},
        project_state::ProjectState,
    };
    Ok(common::edge::EdgeEnd {
        node_type: super::parse_wire(node_type, "node type", NodeType::parse)?,
        status: super::parse_wire(status, "node status", NodeStatus::parse)?,
        project_state: state
            .map(|s| super::parse_wire(s, "project state", ProjectState::parse))
            .transpose()?,
    })
}

impl EdgeWithTitlesRow {
    fn into_edge_with_titles(self) -> Result<EdgeWithTitles, EmberTroveError> {
        let edge_type = parse_edge_type(&self.edge_type)?;
        Ok(EdgeWithTitles {
            id: EdgeId(self.id),
            source_id: NodeId(self.source_id),
            source_title: self.source_title,
            target_id: NodeId(self.target_id),
            target_title: self.target_title,
            // The delete route's own rule, read for this reader.
            may_remove: edge_type.removable_by(self.edits_source, self.edits_target),
            edge_type,
            label: self.label,
            created_at: self.created_at,
            source_node: Some(edge_end(
                &self.source_type,
                &self.source_status,
                self.source_state.as_deref(),
            )?),
            target_node: Some(edge_end(
                &self.target_type,
                &self.target_status,
                self.target_state.as_deref(),
            )?),
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
    Ok(closes_loop(tx, req)
        .await?
        .then(|| EdgeRefusal::Loop(req.edge_type.clone())))
}

/// Link `child` into `parent` (`contains`): its home. The caller has
/// checked access and kinds, and holds the type lock or knows the child is
/// new (no loop can pass through a node with no children). A repeat is a
/// no-op; a vanished node is `NotFound`; a second home (`edges_one_home`)
/// is the usual 422.
pub(crate) async fn insert_home<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    parent: NodeId,
    child: NodeId,
) -> Result<(), EmberTroveError> {
    sqlx::query(
        "INSERT INTO edges (source_id, target_id, edge_type) \
         VALUES ($1, $2, 'contains') ON CONFLICT DO NOTHING",
    )
    .bind(parent.0)
    .bind(child.0)
    .execute(ex)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
            EmberTroveError::NotFound("node not found".to_string())
        }
        sqlx::Error::Database(d) if d.constraint() == Some("edges_one_home") => {
            EmberTroveError::Validation(EdgeRefusal::SecondHome.message())
        }
        _ => EmberTroveError::Internal(format!("set home failed: {e}")),
    })?;
    Ok(())
}

/// SQL for "the link `e` is one of node `n`'s (bound at `$1`) that its
/// editors clear, and its other end `o` is hidden from the reader (bound
/// at `$2`) and from the node's owner" (#349): the one predicate behind
/// the count and the remove, so the two can't disagree.
///
/// The types come from `EdgeType::a_nodes_to_clear`, per direction, in
/// the enum's own wire words (never text from a request). `o` must be
/// joined as the other end: [`OTHER_END`].
///
/// SECURITY: it also holds only for a reader with a grant to **edit**
/// the node (`editable_by`). The route checks that first; the statement
/// holds to it too, so a caller that forgets tells a viewer nothing and
/// removes nothing. An admin (no subject) is shown every link, so
/// nothing is hidden from them either way.
fn not_shown_predicate() -> String {
    let types = |outgoing: bool| {
        let words: Vec<String> = EdgeType::a_nodes_to_clear(outgoing)
            .iter()
            .map(|w| format!("'{w}'"))
            .collect();
        if words.is_empty() {
            "FALSE".to_string()
        } else {
            format!("e.edge_type::text IN ({})", words.join(", "))
        }
    };
    format!(
        "((e.source_id = $1 AND {outgoing}) OR (e.target_id = $1 AND {incoming})) \
         AND {reader_edits} AND NOT {reader_sees} AND NOT {owner_sees}",
        outgoing = types(true),
        incoming = types(false),
        reader_edits = super::editable_by("n.id", 2),
        reader_sees = super::visible_to("o", 2),
        // The owner as a column, in SQL: bound as an optional value, a
        // NULL would read as "sees everything".
        owner_sees = super::owned_or_granted("o", "n.owner_id"),
    )
}

/// The node at the other end of `e` from the node bound at `$1`.
const OTHER_END: &str = "CASE WHEN e.source_id = $1 THEN e.target_id ELSE e.source_id END";

/// Would the edge close a loop: is its source reachable from its target
/// through edges of its type? The caller holds the type's lock.
async fn closes_loop(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    req: &CreateEdgeRequest,
) -> Result<bool, EmberTroveError> {
    let failed = |e: sqlx::Error| EmberTroveError::Internal(format!("edge check failed: {e}"));
    // UNION (not UNION ALL) keeps a legacy loop from recursing forever.
    sqlx::query_scalar(
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
    .map_err(failed)
}

/// Takes the advisory lock for `edge_type` (#224): edges of that type are
/// checked and written one transaction at a time. The cascade takes the
/// `contains` one too (`NodeRepo::set_project_state`), so the tree it
/// walks holds still.
async fn lock_type(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    edge_type: &EdgeType,
) -> Result<(), EmberTroveError> {
    lock_type_raw(tx, edge_type)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("edge lock failed: {e}")))
}

/// [`lock_type`] with the driver's error kept, for a caller that sets a
/// `lock_timeout` and must tell "timed out waiting" from a failure (the
/// cascade).
pub(crate) async fn lock_type_raw(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    edge_type: &EdgeType,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ember.edges.' || $1))")
        .bind(edge_type.as_str())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Every edge. A free function so the backup can run it on its one
/// snapshot transaction (`backup::read_snapshot`).
pub(crate) async fn list_all_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<Edge>, EmberTroveError> {
    let rows = sqlx::query_as::<_, EdgeRow>(
        r#"
        SELECT id, source_id, target_id, edge_type::text, label, created_at
        FROM edges
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list all edges failed: {e}")))?;

    rows.into_iter().map(EdgeRow::into_edge).collect()
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
            lock_type(&mut tx, &req.edge_type).await?;
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
                } else if db_err.constraint() == Some("edges_one_home") {
                    // Migration 042 (#228): the database behind the locked
                    // check above, should any path ever miss it.
                    EmberTroveError::Validation(EdgeRefusal::SecondHome.message())
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

    async fn convert(
        &self,
        id: EdgeId,
        from: EdgeType,
        to: EdgeType,
        swap: bool,
    ) -> Result<(), EmberTroveError> {
        let fail = |e: sqlx::Error| EmberTroveError::Internal(format!("convert edge failed: {e}"));
        let mut tx = self.pool.begin().await.map_err(fail)?;
        // The type is part of the match: if the link changed since the
        // caller's checks, nothing happens (404) rather than a conversion
        // the checks never saw.
        let old: Option<(Uuid, Uuid, Option<String>, DateTime<Utc>)> = sqlx::query_as(
            "DELETE FROM edges WHERE id = $1 AND edge_type = $2::edge_type \
             RETURNING source_id, target_id, label, created_at",
        )
        .bind(id.0)
        .bind(from.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(fail)?;
        let Some((source, target, label, created_at)) = old else {
            return Err(EmberTroveError::NotFound(format!("edge {id} not found")));
        };
        let (source, target) = if swap {
            (target, source)
        } else {
            (source, target)
        };
        // No conflict target: covers both the per-type unique index and
        // `edges_related_to_once`, so an existing link absorbs this one.
        let inserted = sqlx::query(
            "INSERT INTO edges (source_id, target_id, edge_type, label, created_at) \
             VALUES ($1, $2, $3::edge_type, $4, $5) ON CONFLICT DO NOTHING",
        )
        .bind(source)
        .bind(target)
        .bind(to.as_str())
        .bind(&label)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(fail)?
        .rows_affected();
        if inserted == 0 {
            // Absorbed: a blank label on the link that stays takes this
            // one's, the merge rule of migrations 034 and 041.
            sqlx::query(
                "UPDATE edges SET label = $4 \
                 WHERE edge_type = $3::edge_type \
                   AND ((source_id = $1 AND target_id = $2) \
                        OR ($3 = 'related_to' AND source_id = $2 AND target_id = $1)) \
                   AND (label IS NULL OR btrim(label) = '') \
                   AND $4 IS NOT NULL AND btrim($4) <> ''",
            )
            .bind(source)
            .bind(target)
            .bind(to.as_str())
            .bind(&label)
            .execute(&mut *tx)
            .await
            .map_err(fail)?;
        }
        tx.commit().await.map_err(fail)
    }

    async fn home_of(&self, child: NodeId) -> Result<Option<Edge>, EmberTroveError> {
        sqlx::query_as::<_, EdgeRow>(
            "SELECT id, source_id, target_id, edge_type::text, label, created_at \
             FROM edges WHERE edge_type = 'contains' AND target_id = $1",
        )
        .bind(child.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("home lookup failed: {e}")))?
        .map(EdgeRow::into_edge)
        .transpose()
    }

    async fn set_home(
        &self,
        child: NodeId,
        parent: Option<NodeId>,
    ) -> Result<Option<Edge>, EmberTroveError> {
        let fail = |e: sqlx::Error| EmberTroveError::Internal(format!("set home failed: {e}"));
        let mut tx = self.pool.begin().await.map_err(fail)?;
        lock_type(&mut tx, &EdgeType::Contains).await?;
        if let Some(parent) = parent {
            let req = CreateEdgeRequest {
                source_id: parent,
                target_id: child,
                edge_type: EdgeType::Contains,
                label: None,
            };
            // Freeing the old home can't open a way back from the child,
            // so the check needn't wait for it.
            if closes_loop(&mut tx, &req).await? {
                return Err(EmberTroveError::Validation(
                    EdgeRefusal::Loop(EdgeType::Contains).message(),
                ));
            }
        }
        // Every other home goes; the same parent stays as it is.
        sqlx::query(
            "DELETE FROM edges WHERE edge_type = 'contains' AND target_id = $1 \
             AND source_id IS DISTINCT FROM $2",
        )
        .bind(child.0)
        .bind(parent.map(|p| p.0))
        .execute(&mut *tx)
        .await
        .map_err(fail)?;
        let home = match parent {
            None => None,
            Some(parent) => {
                insert_home(&mut *tx, parent, child).await?;
                sqlx::query_as::<_, EdgeRow>(
                    "SELECT id, source_id, target_id, edge_type::text, label, created_at \
                     FROM edges WHERE edge_type = 'contains' AND target_id = $1",
                )
                .bind(child.0)
                .fetch_optional(&mut *tx)
                .await
                .map_err(fail)?
                .map(EdgeRow::into_edge)
                .transpose()?
            }
        };
        tx.commit().await.map_err(fail)?;
        Ok(home)
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
                e.created_at,
                sn.node_type::text AS source_type,
                sn.status::text AS source_status,
                sn.project_state AS source_state,
                tn.node_type::text AS target_type,
                tn.status::text AS target_status,
                tn.project_state AS target_state,
                ($2::text IS NULL OR {edits_source}) AS edits_source,
                ($2::text IS NULL OR {edits_target}) AS edits_target
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
            // Who may remove a link is who may edit an end of it: a grant
            // of their own, as `require_editor` reads it, or an admin
            // (the NULL subject), as that check lets through.
            edits_source = super::editable_by("e.source_id", 2),
            edits_target = super::editable_by("e.target_id", 2),
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

    async fn blockers(
        &self,
        node: NodeId,
        subject: Option<&str>,
    ) -> Result<common::blockers::NodeBlockers, EmberTroveError> {
        use common::blockers::{CHAIN_LIMIT, NodeHop, node_blockers};
        #[derive(sqlx::FromRow)]
        struct Row {
            /// The link's place in the order the walk found it.
            n: i64,
            source_id: Uuid,
            target_id: Uuid,
            title: String,
            node_type: String,
            status: String,
            project_state: Option<String>,
        }
        // The walk's row is the link alone, `(source, target)`, and the
        // two halves are joined with UNION: a loop (older data can hold
        // one) then ends, because there are only so many links. A depth
        // or a path in the row would make every pass a new row and never
        // end.
        //
        // A link is taken when its target is visible to the reader and
        // not met. The walk starts only from a node that still waits (a
        // closed, an archived or a finished one waits for nothing), and
        // goes on only through a node that can still move (`passes_on`).
        //
        // The cut is made on the walk itself, before the titles are
        // joined: the walk hands out its links in the order it finds
        // them (`n`), so what is kept hangs together from the start,
        // where a cut made after the join (which may reorder) could drop
        // a link from the middle and lose everything behind it. One link
        // more than the limit is read to know there is more, and then
        // left out: it is the last one found, so nothing kept hangs on
        // it.
        let rows = sqlx::query_as::<_, Row>(&format!(
            r#"
            WITH RECURSIVE walk(source_id, target_id) AS (
                SELECT e.source_id, e.target_id
                FROM edges e
                JOIN nodes s ON s.id = e.source_id
                JOIN nodes t ON t.id = e.target_id
                WHERE e.source_id = $1
                  AND e.edge_type::text = 'depends_on'
                  AND {s_visible} AND {s_waits}
                  AND {t_visible} AND NOT {t_met}
              UNION
                SELECT e.source_id, e.target_id
                FROM walk w
                JOIN nodes via ON via.id = w.target_id
                JOIN edges e ON e.source_id = w.target_id
                JOIN nodes t ON t.id = e.target_id
                WHERE e.edge_type::text = 'depends_on'
                  AND {via_goes_on}
                  AND {t_visible} AND NOT {t_met}
            ),
            cut AS (
                SELECT row_number() OVER () AS n, source_id, target_id
                FROM walk
                LIMIT $3
            )
            SELECT c.n, c.source_id, c.target_id, t.title,
                   t.node_type::text AS node_type, t.status::text AS status,
                   t.project_state
            FROM cut c
            JOIN nodes t ON t.id = c.target_id
            "#,
            s_visible = super::visible_to("s", 2),
            s_waits = super::node::still_waits("s"),
            t_visible = super::visible_to("t", 2),
            t_met = super::node::satisfies_dependency("t"),
            via_goes_on = super::node::passes_on("via"),
        ))
        .bind(node.0)
        .bind(subject)
        .bind(i64::try_from(CHAIN_LIMIT + 1).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("read blockers failed: {e}")))?;

        let limit = i64::try_from(CHAIN_LIMIT).unwrap_or(i64::MAX);
        let truncated = rows.iter().any(|r| r.n > limit);
        let hops = rows
            .into_iter()
            .filter(|r| r.n <= limit)
            .map(|r| {
                Ok(NodeHop {
                    from: NodeId(r.source_id),
                    to: NodeId(r.target_id),
                    title: r.title,
                    node: edge_end(&r.node_type, &r.status, r.project_state.as_deref())?,
                })
            })
            .collect::<Result<Vec<_>, EmberTroveError>>()?;
        Ok(node_blockers(node, hops, truncated))
    }

    async fn not_shown(&self, node: NodeId, viewer: Option<&str>) -> Result<u32, EmberTroveError> {
        let n: i32 = sqlx::query_scalar(&format!(
            r#"
            SELECT count(*)::int4
            FROM edges e
            JOIN nodes n ON n.id = $1
            JOIN nodes o ON o.id = {OTHER_END}
            WHERE {hidden}
            "#,
            hidden = not_shown_predicate(),
        ))
        .bind(node.0)
        .bind(viewer)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("count links not shown failed: {e}")))?;
        Ok(u32::try_from(n).unwrap_or(0))
    }

    async fn remove_not_shown(
        &self,
        node: NodeId,
        viewer: Option<&str>,
    ) -> Result<u64, EmberTroveError> {
        // No lock and no transaction: taking a link away can't make a
        // loop or a second home, and the statement judges each link as
        // it stands when it runs.
        let result = sqlx::query(&format!(
            r#"
            DELETE FROM edges e
            USING nodes n, nodes o
            WHERE n.id = $1
              AND o.id = {OTHER_END}
              AND {hidden}
            "#,
            hidden = not_shown_predicate(),
        ))
        .bind(node.0)
        .bind(viewer)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("remove links not shown failed: {e}")))?;
        Ok(result.rows_affected())
    }

    async fn list_all(&self) -> Result<Vec<Edge>, EmberTroveError> {
        list_all_on(&self.pool).await
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
