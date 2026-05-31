use async_trait::async_trait;
use common::{EmberTroveError, graph::NodePosition, id::NodeId};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait GraphRepo: Send + Sync {
    /// List node positions. SECURITY: scoped to `subject`'s owned nodes
    /// (`Some(sub)`); `None` (admin) returns all. Prevents enumerating other
    /// owners' node IDs/coordinates.
    async fn list_positions(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<NodePosition>, EmberTroveError>;
    async fn upsert_position(&self, node_id: Uuid, x: f64, y: f64) -> Result<(), EmberTroveError>;
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
struct NodePositionRow {
    node_id: Uuid,
    x: f64,
    y: f64,
}

#[async_trait]
impl GraphRepo for PgGraphRepo {
    async fn list_positions(
        &self,
        subject: Option<&str>,
    ) -> Result<Vec<NodePosition>, EmberTroveError> {
        let rows = sqlx::query_as::<_, NodePositionRow>(
            "SELECT p.node_id, p.x, p.y
             FROM node_positions p
             JOIN nodes n ON n.id = p.node_id
             WHERE ($1::text IS NULL OR n.owner_id = $1::text)",
        )
        .bind(subject)
        .fetch_all(&self.pool)
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

    async fn upsert_position(&self, node_id: Uuid, x: f64, y: f64) -> Result<(), EmberTroveError> {
        sqlx::query(
            "INSERT INTO node_positions (node_id, x, y)
             VALUES ($1, $2, $3)
             ON CONFLICT (node_id) DO UPDATE
             SET x = EXCLUDED.x, y = EXCLUDED.y, updated_at = now()",
        )
        .bind(node_id)
        .bind(x)
        .bind(y)
        .execute(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("upsert_position failed: {e}")))?;

        Ok(())
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
            (Vec::with_capacity(positions.len()),
             Vec::with_capacity(positions.len()),
             Vec::with_capacity(positions.len())),
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
