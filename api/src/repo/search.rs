use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use common::{
    EmberTroveError,
    id::NodeId,
    search::{SearchQuery, SearchResponse, SearchResult},
};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait SearchRepo: Send + Sync {
    /// Search nodes/notes/tasks.
    ///
    /// `subject` scopes results to a single owner (`Some(sub)`); pass `None`
    /// for admins to search across all owners. SECURITY: callers MUST pass the
    /// authenticated subject for non-admins — otherwise search leaks every
    /// owner's private content.
    async fn search(
        &self,
        query: &SearchQuery,
        subject: Option<&str>,
    ) -> Result<SearchResponse, EmberTroveError>;
}

pub struct PgSearchRepo {
    pool: PgPool,
}

impl PgSearchRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Intermediate row type for all search result queries.
#[derive(sqlx::FromRow)]
struct SearchRow {
    id: Uuid,
    title: String,
    slug: String,
    snippet: Option<String>,
    rank: f32,
    node_type: String,
    status: String,
    match_source: Option<String>,
    highlighted_title: Option<String>,
    // Used by SQL `WHERE`/`ORDER BY` (recency); sqlx must map the column, but
    // Rust never reads it after deserialization.
    #[allow(dead_code)]
    updated_at: DateTime<Utc>,
}

impl SearchRow {
    fn into_result(self) -> SearchResult {
        SearchResult {
            node_id: NodeId(self.id),
            title: self.title,
            slug: self.slug,
            snippet: self.snippet,
            rank: self.rank,
            node_type: self.node_type,
            status: self.status,
            match_source: self.match_source,
            highlighted_title: self.highlighted_title,
        }
    }
}

/// Count-only row for the total-count subquery.
#[derive(sqlx::FromRow)]
struct CountRow {
    total: i64,
}

/// Parse the `tag_ids` comma-separated string from the query into a Vec<Uuid>.
fn parse_tag_ids(tag_ids: &Option<String>) -> Vec<Uuid> {
    tag_ids
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| Uuid::parse_str(s.trim()).ok())
        .collect()
}

/// Returns a static ORDER BY clause string.
///
/// `has_rank` should be `true` for full-text and fuzzy searches (where each
/// result carries a meaningful rank score) and `false` for browse mode.
/// All returned strings are compile-time constants — never user input.
fn sort_clause(sort: &Option<String>, has_rank: bool) -> &'static str {
    match sort.as_deref() {
        Some("updated_desc") => "ORDER BY updated_at DESC, title",
        Some("updated_asc") => "ORDER BY updated_at ASC, title",
        Some("title_asc") => "ORDER BY title ASC",
        Some("title_desc") => "ORDER BY title DESC",
        _ => {
            if has_rank {
                "ORDER BY rank DESC, title"
            } else {
                "ORDER BY updated_at DESC, title"
            }
        }
    }
}

#[async_trait]
impl SearchRepo for PgSearchRepo {
    async fn search(
        &self,
        query: &SearchQuery,
        subject: Option<&str>,
    ) -> Result<SearchResponse, EmberTroveError> {
        let q = query.q.trim();
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query.per_page.unwrap_or(20).min(100);
        let offset = (page - 1) * per_page;
        let tag_ids = parse_tag_ids(&query.tag_ids);
        let and_mode = query.tag_op.as_deref() == Some("and");

        // Convert NaiveDate bounds to UTC datetime bounds.
        // updated_after → start of that day (00:00:00 UTC)
        // updated_before → start of the *next* day (exclusive upper bound)
        let after_dt: Option<DateTime<Utc>> = query
            .updated_after
            .map(|d| d.and_time(NaiveTime::MIN).and_utc());
        let before_dt: Option<DateTime<Utc>> = query
            .updated_before
            .and_then(|d| d.succ_opt())
            .map(|d| d.and_time(NaiveTime::MIN).and_utc());

        // Empty query → list all nodes (optionally filtered).
        if q.is_empty() {
            return self
                .list_nodes(
                    &query.node_type,
                    &query.status,
                    &tag_ids,
                    and_mode,
                    &query.sort,
                    &after_dt,
                    &before_dt,
                    page,
                    per_page,
                    offset,
                    subject,
                )
                .await;
        }

        let fuzzy = query.fuzzy.unwrap_or(false);

        if fuzzy {
            self.fuzzy_search(
                q,
                &query.node_type,
                &query.status,
                &tag_ids,
                and_mode,
                &query.sort,
                &after_dt,
                &before_dt,
                page,
                per_page,
                offset,
                subject,
            )
            .await
        } else {
            self.fulltext_search(
                q,
                &query.node_type,
                &query.status,
                &tag_ids,
                and_mode,
                &query.sort,
                &after_dt,
                &before_dt,
                page,
                per_page,
                offset,
                subject,
            )
            .await
        }
    }
}

impl PgSearchRepo {
    /// List nodes with optional filters — used for empty-query browsing.
    ///
    /// Tag filter SQL uses `= ANY($n::uuid[])` with a HAVING clause to express
    /// AND/OR without dynamic query construction.
    /// An empty array means no tag filter (`array_length = NULL`).
    #[allow(clippy::too_many_arguments)]
    async fn list_nodes(
        &self,
        node_type: &Option<common::node::NodeType>,
        status: &Option<common::node::NodeStatus>,
        tag_ids: &[Uuid],
        and_mode: bool,
        sort: &Option<String>,
        updated_after: &Option<DateTime<Utc>>,
        updated_before: &Option<DateTime<Utc>>,
        page: u32,
        per_page: u32,
        offset: u32,
        subject: Option<&str>,
    ) -> Result<SearchResponse, EmberTroveError> {
        let type_filter = node_type.as_ref().map(node_type_to_str);
        let status_filter = status.as_ref().map(node_status_to_str);

        let count_row = sqlx::query_as::<_, CountRow>(
            r#"
            SELECT COUNT(*)::bigint AS total
            FROM nodes
            WHERE ($1::text IS NULL OR node_type = $1::node_type)
              AND ($2::text IS NULL OR status = $2::node_status)
              AND (
                array_length($3::uuid[], 1) IS NULL
                OR id IN (
                    SELECT node_id FROM node_tags
                    WHERE tag_id = ANY($3::uuid[])
                    GROUP BY node_id
                    HAVING (NOT $4) OR COUNT(DISTINCT tag_id) = array_length($3::uuid[], 1)
                )
              )
              AND ($5::timestamptz IS NULL OR updated_at >= $5)
              AND ($6::timestamptz IS NULL OR updated_at < $6)
              AND ($7::text IS NULL OR owner_id = $7::text)
            "#,
        )
        .bind(type_filter)
        .bind(status_filter)
        .bind(tag_ids)
        .bind(and_mode)
        .bind(updated_after)
        .bind(updated_before)
        .bind(subject)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list nodes count failed: {e}")))?;

        let rows = sqlx::query_as::<_, SearchRow>(&format!(
            r#"
                SELECT
                    id,
                    title,
                    slug,
                    NULL::text AS snippet,
                    1.0::float4 AS rank,
                    node_type::text AS node_type,
                    status::text AS status,
                    NULL::text AS match_source,
                    NULL::text AS highlighted_title,
                    updated_at
                FROM nodes
                WHERE ($1::text IS NULL OR node_type = $1::node_type)
                  AND ($2::text IS NULL OR status = $2::node_status)
                  AND (
                    array_length($3::uuid[], 1) IS NULL
                    OR id IN (
                        SELECT node_id FROM node_tags
                        WHERE tag_id = ANY($3::uuid[])
                        GROUP BY node_id
                        HAVING (NOT $4) OR COUNT(DISTINCT tag_id) = array_length($3::uuid[], 1)
                    )
                  )
                  AND ($5::timestamptz IS NULL OR updated_at >= $5)
                  AND ($6::timestamptz IS NULL OR updated_at < $6)
                  AND ($9::text IS NULL OR owner_id = $9::text)
                {order}
                LIMIT $7 OFFSET $8
                "#,
            order = sort_clause(sort, false),
        ))
        .bind(type_filter)
        .bind(status_filter)
        .bind(tag_ids)
        .bind(and_mode)
        .bind(updated_after)
        .bind(updated_before)
        .bind(per_page as i64)
        .bind(offset as i64)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list nodes failed: {e}")))?;

        Ok(SearchResponse {
            results: rows.into_iter().map(SearchRow::into_result).collect(),
            total: count_row.total as u64,
            page,
            per_page,
        })
    }

    /// PostgreSQL full-text search across node title/body, note bodies, and task titles.
    ///
    /// Uses a `UNION ALL` of three candidate sets (node / note / task), then
    /// `DISTINCT ON (id)` to deduplicate by node, keeping the highest-ranked
    /// source.  A `valid_nodes` CTE applies type/status/tag/date filters once
    /// so they are not repeated in every branch.
    ///
    /// Title highlighting: the node branch generates a second `ts_headline` call
    /// on `n.title` so the UI can render matched words in the title in bold.
    /// Note/task branches emit `NULL` for `highlighted_title`.
    #[allow(clippy::too_many_arguments)]
    async fn fulltext_search(
        &self,
        q: &str,
        node_type: &Option<common::node::NodeType>,
        status: &Option<common::node::NodeStatus>,
        tag_ids: &[Uuid],
        and_mode: bool,
        sort: &Option<String>,
        updated_after: &Option<DateTime<Utc>>,
        updated_before: &Option<DateTime<Utc>>,
        page: u32,
        per_page: u32,
        offset: u32,
        subject: Option<&str>,
    ) -> Result<SearchResponse, EmberTroveError> {
        let type_filter = node_type.as_ref().map(node_type_to_str);
        let status_filter = status.as_ref().map(node_status_to_str);

        let count_row = sqlx::query_as::<_, CountRow>(
            r#"
            WITH valid_nodes AS (
                SELECT id FROM nodes
                WHERE ($2::text IS NULL OR node_type = $2::node_type)
                  AND ($3::text IS NULL OR status = $3::node_status)
                  AND (
                    array_length($4::uuid[], 1) IS NULL
                    OR id IN (
                        SELECT node_id FROM node_tags
                        WHERE tag_id = ANY($4::uuid[])
                        GROUP BY node_id
                        HAVING (NOT $5) OR COUNT(DISTINCT tag_id) = array_length($4::uuid[], 1)
                    )
                  )
                  AND ($6::timestamptz IS NULL OR updated_at >= $6)
                  AND ($7::timestamptz IS NULL OR updated_at < $7)
                  AND ($8::text IS NULL OR owner_id = $8::text)
            )
            SELECT COUNT(DISTINCT n.id)::bigint AS total
            FROM nodes n
            WHERE n.id IN (SELECT id FROM valid_nodes)
              AND (
                n.search_vec @@ websearch_to_tsquery('english', $1)
                OR EXISTS (
                    SELECT 1 FROM node_notes nn
                    WHERE nn.node_id = n.id
                      AND nn.search_vec @@ websearch_to_tsquery('english', $1)
                )
                OR EXISTS (
                    SELECT 1 FROM node_tasks nt
                    WHERE nt.node_id = n.id
                      AND to_tsvector('english', nt.title) @@ websearch_to_tsquery('english', $1)
                )
              )
            "#,
        )
        .bind(q)
        .bind(type_filter)
        .bind(status_filter)
        .bind(tag_ids)
        .bind(and_mode)
        .bind(updated_after)
        .bind(updated_before)
        .bind(subject)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("search count failed: {e}")))?;

        let rows = sqlx::query_as::<_, SearchRow>(
            &format!(
                r#"
                WITH valid_nodes AS (
                    SELECT id FROM nodes
                    WHERE ($2::text IS NULL OR node_type = $2::node_type)
                      AND ($3::text IS NULL OR status = $3::node_status)
                      AND (
                        array_length($4::uuid[], 1) IS NULL
                        OR id IN (
                            SELECT node_id FROM node_tags
                            WHERE tag_id = ANY($4::uuid[])
                            GROUP BY node_id
                            HAVING (NOT $5) OR COUNT(DISTINCT tag_id) = array_length($4::uuid[], 1)
                        )
                      )
                      AND ($6::timestamptz IS NULL OR updated_at >= $6)
                      AND ($7::timestamptz IS NULL OR updated_at < $7)
                      AND ($10::text IS NULL OR owner_id = $10::text)
                ),
                candidates AS (
                    -- Match from node title / body
                    -- normalization=1: divide by log(unique words) to prevent long-body
                    -- documents from unfairly outranking short, well-titled nodes.
                    SELECT
                        n.id,
                        n.title,
                        n.slug,
                        ts_headline(
                            'english',
                            coalesce(n.body, ''),
                            websearch_to_tsquery('english', $1),
                            'StartSel=<mark>, StopSel=</mark>, MaxFragments=2, MaxWords=30, MinWords=10'
                        ) AS snippet,
                        ts_rank_cd(n.search_vec, websearch_to_tsquery('english', $1), 1) AS rank,
                        n.node_type::text AS node_type,
                        n.status::text AS status,
                        'node'::text AS match_source,
                        ts_headline(
                            'english',
                            n.title,
                            websearch_to_tsquery('english', $1),
                            'StartSel=<mark>, StopSel=</mark>, MaxFragments=1, MinWords=1, MaxWords=10'
                        ) AS highlighted_title
                    FROM nodes n
                    WHERE n.id IN (SELECT id FROM valid_nodes)
                      AND n.search_vec @@ websearch_to_tsquery('english', $1)

                    UNION ALL

                    -- Match from an attached note body
                    SELECT
                        n.id,
                        n.title,
                        n.slug,
                        ts_headline(
                            'english',
                            nn.body,
                            websearch_to_tsquery('english', $1),
                            'StartSel=<mark>, StopSel=</mark>, MaxFragments=2, MaxWords=30, MinWords=10'
                        ) AS snippet,
                        ts_rank_cd(nn.search_vec, websearch_to_tsquery('english', $1), 1) AS rank,
                        n.node_type::text AS node_type,
                        n.status::text AS status,
                        'note'::text AS match_source,
                        NULL::text AS highlighted_title
                    FROM node_notes nn
                    JOIN nodes n ON n.id = nn.node_id
                    WHERE n.id IN (SELECT id FROM valid_nodes)
                      AND nn.search_vec @@ websearch_to_tsquery('english', $1)

                    UNION ALL

                    -- Match from an attached task title
                    SELECT
                        n.id,
                        n.title,
                        n.slug,
                        substring(nt.title FROM 1 FOR 200) AS snippet,
                        ts_rank_cd(
                            to_tsvector('english', nt.title),
                            websearch_to_tsquery('english', $1),
                            1
                        ) AS rank,
                        n.node_type::text AS node_type,
                        n.status::text AS status,
                        'task'::text AS match_source,
                        NULL::text AS highlighted_title
                    FROM node_tasks nt
                    JOIN nodes n ON n.id = nt.node_id
                    WHERE n.id IN (SELECT id FROM valid_nodes)
                      AND to_tsvector('english', nt.title) @@ websearch_to_tsquery('english', $1)
                ),
                best AS (
                    SELECT DISTINCT ON (id)
                        id, title, slug, snippet, rank, node_type, status, match_source,
                        highlighted_title
                    FROM candidates
                    ORDER BY id, rank DESC
                )
                SELECT
                    b.id, b.title, b.slug, b.snippet, b.rank,
                    b.node_type, b.status, b.match_source, b.highlighted_title,
                    n.updated_at
                FROM best b
                JOIN nodes n ON n.id = b.id
                {order}
                LIMIT $8 OFFSET $9
                "#,
                order = sort_clause(sort, true),
            ),
        )
        .bind(q)
        .bind(type_filter)
        .bind(status_filter)
        .bind(tag_ids)
        .bind(and_mode)
        .bind(updated_after)
        .bind(updated_before)
        .bind(per_page as i64)
        .bind(offset as i64)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("fulltext search failed: {e}")))?;

        Ok(SearchResponse {
            results: rows.into_iter().map(SearchRow::into_result).collect(),
            total: count_row.total as u64,
            page,
            per_page,
        })
    }

    /// Fuzzy trigram search across node title/body, note bodies, and task titles.
    ///
    /// Uses `similarity()` / `ILIKE` predicates with the same `UNION ALL` +
    /// `DISTINCT ON` deduplication strategy as `fulltext_search`.
    #[allow(clippy::too_many_arguments)]
    async fn fuzzy_search(
        &self,
        q: &str,
        node_type: &Option<common::node::NodeType>,
        status: &Option<common::node::NodeStatus>,
        tag_ids: &[Uuid],
        and_mode: bool,
        sort: &Option<String>,
        updated_after: &Option<DateTime<Utc>>,
        updated_before: &Option<DateTime<Utc>>,
        page: u32,
        per_page: u32,
        offset: u32,
        subject: Option<&str>,
    ) -> Result<SearchResponse, EmberTroveError> {
        let type_filter = node_type.as_ref().map(node_type_to_str);
        let status_filter = status.as_ref().map(node_status_to_str);
        let like_pattern = format!("%{q}%");

        let count_row = sqlx::query_as::<_, CountRow>(
            r#"
            WITH valid_nodes AS (
                SELECT id FROM nodes
                WHERE ($3::text IS NULL OR node_type = $3::node_type)
                  AND ($4::text IS NULL OR status = $4::node_status)
                  AND (
                    array_length($5::uuid[], 1) IS NULL
                    OR id IN (
                        SELECT node_id FROM node_tags
                        WHERE tag_id = ANY($5::uuid[])
                        GROUP BY node_id
                        HAVING (NOT $6) OR COUNT(DISTINCT tag_id) = array_length($5::uuid[], 1)
                    )
                  )
                  AND ($7::timestamptz IS NULL OR updated_at >= $7)
                  AND ($8::timestamptz IS NULL OR updated_at < $8)
                  AND ($9::text IS NULL OR owner_id = $9::text)
            )
            SELECT COUNT(DISTINCT n.id)::bigint AS total
            FROM nodes n
            WHERE n.id IN (SELECT id FROM valid_nodes)
              AND (
                similarity(n.title, $1) > 0.1
                OR n.body ILIKE $2
                OR EXISTS (
                    SELECT 1 FROM node_notes nn
                    WHERE nn.node_id = n.id AND nn.body ILIKE $2
                )
                OR EXISTS (
                    SELECT 1 FROM node_tasks nt
                    WHERE nt.node_id = n.id
                      AND (similarity(nt.title, $1) > 0.1 OR nt.title ILIKE $2)
                )
              )
            "#,
        )
        .bind(q)
        .bind(&like_pattern)
        .bind(type_filter)
        .bind(status_filter)
        .bind(tag_ids)
        .bind(and_mode)
        .bind(updated_after)
        .bind(updated_before)
        .bind(subject)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("fuzzy count failed: {e}")))?;

        let rows = sqlx::query_as::<_, SearchRow>(&format!(
            r#"
                WITH valid_nodes AS (
                    SELECT id FROM nodes
                    WHERE ($3::text IS NULL OR node_type = $3::node_type)
                      AND ($4::text IS NULL OR status = $4::node_status)
                      AND (
                        array_length($5::uuid[], 1) IS NULL
                        OR id IN (
                            SELECT node_id FROM node_tags
                            WHERE tag_id = ANY($5::uuid[])
                            GROUP BY node_id
                            HAVING (NOT $6) OR COUNT(DISTINCT tag_id) = array_length($5::uuid[], 1)
                        )
                      )
                      AND ($7::timestamptz IS NULL OR updated_at >= $7)
                      AND ($8::timestamptz IS NULL OR updated_at < $8)
                      AND ($11::text IS NULL OR owner_id = $11::text)
                ),
                candidates AS (
                    -- Match from node title / body
                    SELECT
                        n.id,
                        n.title,
                        n.slug,
                        CASE WHEN n.body ILIKE $2
                            THEN substring(n.body FROM 1 FOR 200)
                            ELSE NULL
                        END AS snippet,
                        GREATEST(
                            similarity(n.title, $1),
                            CASE WHEN n.body ILIKE $2 THEN 0.05 ELSE 0.0 END
                        ) AS rank,
                        n.node_type::text AS node_type,
                        n.status::text AS status,
                        'node'::text AS match_source,
                        NULL::text AS highlighted_title
                    FROM nodes n
                    WHERE n.id IN (SELECT id FROM valid_nodes)
                      AND (similarity(n.title, $1) > 0.1 OR n.body ILIKE $2)

                    UNION ALL

                    -- Match from an attached note body
                    SELECT
                        n.id,
                        n.title,
                        n.slug,
                        substring(nn.body FROM 1 FOR 200) AS snippet,
                        0.1::float4 AS rank,
                        n.node_type::text AS node_type,
                        n.status::text AS status,
                        'note'::text AS match_source,
                        NULL::text AS highlighted_title
                    FROM node_notes nn
                    JOIN nodes n ON n.id = nn.node_id
                    WHERE n.id IN (SELECT id FROM valid_nodes)
                      AND nn.body ILIKE $2

                    UNION ALL

                    -- Match from an attached task title
                    SELECT
                        n.id,
                        n.title,
                        n.slug,
                        substring(nt.title FROM 1 FOR 200) AS snippet,
                        GREATEST(similarity(nt.title, $1), 0.0) AS rank,
                        n.node_type::text AS node_type,
                        n.status::text AS status,
                        'task'::text AS match_source,
                        NULL::text AS highlighted_title
                    FROM node_tasks nt
                    JOIN nodes n ON n.id = nt.node_id
                    WHERE n.id IN (SELECT id FROM valid_nodes)
                      AND (similarity(nt.title, $1) > 0.1 OR nt.title ILIKE $2)
                ),
                best AS (
                    SELECT DISTINCT ON (id)
                        id, title, slug, snippet, rank, node_type, status, match_source,
                        highlighted_title
                    FROM candidates
                    ORDER BY id, rank DESC
                )
                SELECT
                    b.id, b.title, b.slug, b.snippet, b.rank,
                    b.node_type, b.status, b.match_source, b.highlighted_title,
                    n.updated_at
                FROM best b
                JOIN nodes n ON n.id = b.id
                {order}
                LIMIT $9 OFFSET $10
                "#,
            order = sort_clause(sort, true),
        ))
        .bind(q)
        .bind(&like_pattern)
        .bind(type_filter)
        .bind(status_filter)
        .bind(tag_ids)
        .bind(and_mode)
        .bind(updated_after)
        .bind(updated_before)
        .bind(per_page as i64)
        .bind(offset as i64)
        .bind(subject)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("fuzzy search failed: {e}")))?;

        Ok(SearchResponse {
            results: rows.into_iter().map(SearchRow::into_result).collect(),
            total: count_row.total as u64,
            page,
            per_page,
        })
    }
}

fn node_type_to_str(t: &common::node::NodeType) -> &'static str {
    match t {
        common::node::NodeType::Article => "article",
        common::node::NodeType::Project => "project",
        common::node::NodeType::Area => "area",
        common::node::NodeType::Resource => "resource",
        common::node::NodeType::Reference => "reference",
    }
}

fn node_status_to_str(s: &common::node::NodeStatus) -> &'static str {
    match s {
        common::node::NodeStatus::Draft => "draft",
        common::node::NodeStatus::Published => "published",
        common::node::NodeStatus::Archived => "archived",
    }
}
