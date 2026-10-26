use common::EmberTroveError;

pub mod activity;
pub mod attachment;
pub mod backup;
pub mod edge;
pub mod editor_pref;
pub mod favorite;
pub mod graph;
pub mod node;
pub mod node_link;
pub mod node_version;
pub mod note;
pub mod permission;
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
