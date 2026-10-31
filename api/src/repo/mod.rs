use common::EmberTroveError;

pub mod activity;
pub mod attachment;
pub mod backup;
pub mod data_health;
pub mod edge;
pub mod editor_pref;
pub mod favorite;
pub mod graph;
pub mod node;
pub mod node_link;
pub mod node_version;
pub mod note;
pub mod permission;
pub mod person;
#[cfg(all(test, feature = "pg-tests"))]
mod pg_tests;
pub mod pkce;
pub mod search;
pub mod search_presets;
pub mod tag;
pub mod task;
pub mod template;
pub mod webhook;

/// SQL predicate: the node aliased `alias` is visible to the subject bound at
/// placeholder `$param` — owned by them, or shared with them through the
/// `permissions` table. A NULL subject (admin) sees every node. The same rule
/// as `NodeRepo::list_titles`.
///
/// SECURITY: `alias` and `param` must be compile-time literals (callers pass
/// constants); the subject itself is always a bind parameter.
pub(crate) fn visible_to(alias: &str, param: u8) -> String {
    format!(
        "(${param}::text IS NULL OR {alias}.owner_id = ${param}::text OR EXISTS ( \
         SELECT 1 FROM permissions vp WHERE vp.node_id = {alias}.id AND vp.subject_id = ${param}::text))"
    )
}

/// Escape SQL `LIKE`/`ILIKE` metacharacters (`\`, `%`, `_`) so a user-supplied
/// substring is matched literally. Pair with `ESCAPE '\'` in the query.
/// Shared by the notes feed and fuzzy search.
pub(crate) fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Parse a Postgres enum column through its `common` wire table
/// (`wire_enum!`). An unknown value means the schema and the code disagree —
/// a server-side invariant break, reported as Internal, never client input.
pub(crate) fn parse_wire<T>(
    s: &str,
    kind: &str,
    parse: fn(&str) -> Option<T>,
) -> Result<T, EmberTroveError> {
    parse(s).ok_or_else(|| EmberTroveError::Internal(format!("unknown {kind}: {s}")))
}

/// Whether this session is inside a transaction block. There, the
/// transaction began before the current statement; outside one, each
/// statement is its own transaction and the two start times are equal.
/// Sent as a bare `&str`, so over the simple query protocol: through the
/// extended protocol (Parse/Bind/Execute, what `sqlx::query` uses) the two
/// times differ even outside a block, which would read every connection as
/// stuck. The pg test checks both directions.
pub(crate) async fn in_transaction_block(
    conn: &mut sqlx::PgConnection,
) -> Result<bool, sqlx::Error> {
    use sqlx::{Executor, Row};
    conn.fetch_one(
        "SELECT xact_start IS DISTINCT FROM query_start \
         FROM pg_stat_activity WHERE pid = pg_backend_pid()",
    )
    .await?
    .try_get(0)
}

/// The API's connection pool settings (#234).
///
/// A connection must never return to the pool inside a transaction. sqlx
/// 0.8 leaves it there when a request is cancelled (the client went away)
/// while `BEGIN` is in flight: its rollback guard checks a depth counter
/// that isn't incremented yet, so no `ROLLBACK` is queued. Every later
/// request handed that connection would then run inside a transaction no
/// one commits: its writes vanish, and its row locks block other writers.
/// The e2e suite hit exactly this (a node created but never committed).
/// So each release asks the server, and a connection still in a
/// transaction is closed instead of reused; the server then rolls that
/// transaction back. One small query per release is the cost.
#[must_use]
pub fn pool_options() -> sqlx::postgres::PgPoolOptions {
    sqlx::postgres::PgPoolOptions::new().after_release(|conn, _meta| release_check(conn))
}

/// `after_release`: keep the connection unless it's inside a transaction.
fn release_check(
    conn: &mut sqlx::PgConnection,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
    Box::pin(async move {
        let in_block = in_transaction_block(conn).await?;
        if in_block {
            tracing::warn!("closing a pooled connection left inside a transaction");
        }
        Ok(!in_block)
    })
}
