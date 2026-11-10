//! Project Briefs (#282).
//!
//! SECURITY: a Brief belongs to its node, so this repo is keyed by node and
//! never decides access: the routes check viewer (read) or editor (write)
//! rights first, and the dashboard only asks for nodes it already scoped
//! to the caller. Every value is a bind parameter. Brief text is user
//! content: it never goes into an error message or a log line.
//!
//! A save and the History entry for the Brief it replaces commit together
//! in one transaction holding the Brief's row lock, so a supersede can
//! neither lose the old words nor record them twice.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    activity::ActivityAction,
    brief::{BriefFields, BriefSave, ProjectBrief, save_mode, superseded_metadata},
    id::NodeId,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::activity::{ActivityActor, record_in};

#[async_trait]
pub trait BriefRepo: Send + Sync {
    /// The node's current Brief, if it has one.
    async fn get(&self, node: NodeId) -> Result<Option<ProjectBrief>, EmberTroveError>;
    /// The current Briefs of `nodes` (the dashboard; the caller scoped them).
    async fn for_nodes(&self, nodes: &[NodeId]) -> Result<Vec<ProjectBrief>, EmberTroveError>;
    /// Save `fields` as the node's Brief by the owner's supersede rule
    /// (`common::brief::save_mode`). `expected` is the `updated_at` the
    /// editor started from: `None` means "there is no Brief yet". A
    /// mismatch either way is `PreconditionFailed` (412).
    async fn save(
        &self,
        node: NodeId,
        fields: &BriefFields,
        expected: Option<DateTime<Utc>>,
        actor: &ActivityActor,
    ) -> Result<(ProjectBrief, BriefSave), EmberTroveError>;
    /// Remove the node's Brief; its words go to History first. NotFound
    /// when there is none, `PreconditionFailed` when it changed since
    /// `expected`.
    async fn delete(
        &self,
        node: NodeId,
        expected: Option<DateTime<Utc>>,
        actor: &ActivityActor,
    ) -> Result<(), EmberTroveError>;
    /// Every Brief, for the admin backup.
    async fn list_all(&self) -> Result<Vec<ProjectBrief>, EmberTroveError>;
}

pub struct PgBriefRepo {
    pool: PgPool,
}

impl PgBriefRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    node_id: Uuid,
    headline: Option<String>,
    pitch: String,
    points: Vec<String>,
    author_id: String,
    stated_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for ProjectBrief {
    fn from(r: Row) -> Self {
        Self {
            node_id: NodeId(r.node_id),
            headline: r.headline,
            pitch: r.pitch,
            points: r.points,
            author_id: r.author_id,
            stated_at: r.stated_at,
            updated_at: r.updated_at,
        }
    }
}

const COLUMNS: &str = "node_id, headline, pitch, points, author_id, stated_at, updated_at";

/// No content and no subject in the message: it may reach the logs.
fn db(what: &'static str) -> impl Fn(sqlx::Error) -> EmberTroveError {
    move |e| EmberTroveError::Internal(format!("brief {what} failed: {e}"))
}

fn stale() -> EmberTroveError {
    EmberTroveError::PreconditionFailed("the brief changed since you opened it".to_string())
}

/// The current Brief, row-locked for the rest of the transaction.
///
/// The node is locked first (`FOR KEY SHARE`, the lock the History
/// entry's foreign key takes anyway). A node delete, an account purge and a
/// restore all lock the node and then reach the Brief through the cascade;
/// taking the Brief first could deadlock with them, and Postgres would
/// abort one side as a 500. A node that is gone is a clean 404.
async fn locked(
    conn: &mut PgConnection,
    node: NodeId,
) -> Result<Option<ProjectBrief>, EmberTroveError> {
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM nodes WHERE id = $1 FOR KEY SHARE")
            .bind(node.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db("lock node"))?;
    if exists.is_none() {
        return Err(EmberTroveError::NotFound("node not found".to_string()));
    }
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM project_briefs WHERE node_id = $1 FOR UPDATE"
    ))
    .bind(node.0)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db("lock"))?;
    Ok(row.map(Into::into))
}

/// Writes the History entry for a Brief that is being replaced or removed.
async fn record_superseded(
    conn: &mut PgConnection,
    old: &ProjectBrief,
    actor: &ActivityActor,
    removed: bool,
) -> Result<(), EmberTroveError> {
    let mut meta = superseded_metadata(old);
    if removed && let Some(obj) = meta.as_object_mut() {
        obj.insert("removed".to_string(), serde_json::Value::Bool(true));
    }
    record_in(
        conn,
        old.node_id,
        &actor.subject_id,
        ActivityAction::BriefSuperseded,
        actor.metadata_with(meta),
    )
    .await
}

#[async_trait]
impl BriefRepo for PgBriefRepo {
    async fn get(&self, node: NodeId) -> Result<Option<ProjectBrief>, EmberTroveError> {
        let row: Option<Row> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM project_briefs WHERE node_id = $1"
        ))
        .bind(node.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(db("read"))?;
        Ok(row.map(Into::into))
    }

    async fn for_nodes(&self, nodes: &[NodeId]) -> Result<Vec<ProjectBrief>, EmberTroveError> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = nodes.iter().map(|n| n.0).collect();
        let rows: Vec<Row> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM project_briefs WHERE node_id = ANY($1)"
        ))
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(db("batch read"))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn save(
        &self,
        node: NodeId,
        fields: &BriefFields,
        expected: Option<DateTime<Utc>>,
        actor: &ActivityActor,
    ) -> Result<(ProjectBrief, BriefSave), EmberTroveError> {
        let mut tx = self.pool.begin().await.map_err(db("begin"))?;
        let current = locked(&mut tx, node).await?;
        if current.as_ref().map(|c| c.updated_at) != expected {
            return Err(stale());
        }
        let mode = save_mode(current.as_ref(), fields, &actor.subject_id, Utc::now());
        let row: Option<Row> = match (mode, current) {
            (BriefSave::Unchanged, Some(cur)) => return Ok((cur, mode)),
            (BriefSave::InPlace, Some(_)) => sqlx::query_as(&format!(
                "UPDATE project_briefs
                 SET headline = $2, pitch = $3, points = $4, updated_at = clock_timestamp()
                 WHERE node_id = $1
                 RETURNING {COLUMNS}"
            ))
            .bind(node.0)
            .bind(&fields.headline)
            .bind(&fields.pitch)
            .bind(&fields.points)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db("update"))?,
            (BriefSave::Supersede, Some(cur)) => {
                record_superseded(&mut tx, &cur, actor, false).await?;
                sqlx::query_as(&format!(
                    "UPDATE project_briefs
                     SET headline = $2, pitch = $3, points = $4, author_id = $5,
                         stated_at = clock_timestamp(), updated_at = clock_timestamp()
                     WHERE node_id = $1
                     RETURNING {COLUMNS}"
                ))
                .bind(node.0)
                .bind(&fields.headline)
                .bind(&fields.pitch)
                .bind(&fields.points)
                .bind(&actor.subject_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db("supersede"))?
            }
            // No Brief yet. A concurrent first save holds the key: the
            // loser gets 412 like any other lost race, not a 500.
            _ => sqlx::query_as(&format!(
                "INSERT INTO project_briefs
                     (node_id, headline, pitch, points, author_id, stated_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, clock_timestamp(), clock_timestamp())
                 ON CONFLICT (node_id) DO NOTHING
                 RETURNING {COLUMNS}"
            ))
            .bind(node.0)
            .bind(&fields.headline)
            .bind(&fields.pitch)
            .bind(&fields.points)
            .bind(&actor.subject_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| match &e {
                // The node was deleted meanwhile.
                sqlx::Error::Database(d) if d.is_foreign_key_violation() => {
                    EmberTroveError::NotFound("node not found".to_string())
                }
                _ => db("insert")(e),
            })?,
        };
        let brief: ProjectBrief = row.ok_or_else(stale)?.into();
        tx.commit().await.map_err(db("commit"))?;
        Ok((brief, mode))
    }

    async fn delete(
        &self,
        node: NodeId,
        expected: Option<DateTime<Utc>>,
        actor: &ActivityActor,
    ) -> Result<(), EmberTroveError> {
        let mut tx = self.pool.begin().await.map_err(db("begin"))?;
        let Some(cur) = locked(&mut tx, node).await? else {
            return Err(EmberTroveError::NotFound("brief not found".to_string()));
        };
        // Like a save: the caller names the version it is removing. A
        // delete with none would remove words someone else just stated.
        if expected != Some(cur.updated_at) {
            return Err(stale());
        }
        record_superseded(&mut tx, &cur, actor, true).await?;
        sqlx::query("DELETE FROM project_briefs WHERE node_id = $1")
            .bind(node.0)
            .execute(&mut *tx)
            .await
            .map_err(db("delete"))?;
        tx.commit().await.map_err(db("commit"))?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<ProjectBrief>, EmberTroveError> {
        list_all_on(&self.pool).await
    }
}

/// Every Brief. A free function so the backup can run it on its one
/// snapshot transaction (`backup::read_snapshot`; #281): a table read
/// outside that snapshot can disagree with the nodes it points at.
pub(crate) async fn list_all_on<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<ProjectBrief>, EmberTroveError> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM project_briefs ORDER BY node_id"
    ))
    .fetch_all(ex)
    .await
    .map_err(db("list"))?;
    Ok(rows.into_iter().map(Into::into).collect())
}
