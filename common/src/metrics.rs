//! Operational metrics for the admin Performance page (performance phase,
//! 2026-11-02).
//!
//! `GET /api/metrics` returns [`Metrics`]: request counts and latency per
//! route since the API started. `GET /api/metrics/db` returns
//! [`DbStatements`]: the database's slowest statements from
//! `pg_stat_statements`. Both are admin-only.
//!
//! Nothing here carries user data. A route is its matched pattern
//! (`GET /api/nodes/{id}`), never a real path or query. A statement is the
//! text Postgres normalised (`$1` for every value), with its comments
//! stripped ([`strip_sql_comments`]); its table and column names stay.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The `GET /api/metrics` snapshot. Everything is since the API started, so
/// it resets on every deploy.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct Metrics {
    pub version: String,
    pub uptime_secs: u64,
    pub db: DbMetrics,
    pub counts: EntityCounts,
    pub requests: RequestCounts,
    /// The largest total time first.
    pub routes: Vec<RouteLatency>,
}

/// One route's latency since start, in ms (0.1 ms resolution). Percentiles
/// are bucket bounds, good to one bucket and never above `max_ms`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct RouteLatency {
    /// Method and matched pattern, e.g. `GET /api/nodes/{id}`.
    pub route: String,
    pub count: u64,
    /// Time spent on this route in all. Defaults to 0 so a response from
    /// before the field (the service worker's offline copy) still parses.
    #[serde(default)]
    pub total_ms: f64,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct RequestCounts {
    pub total: u64,
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct DbMetrics {
    pub pool_size: u32,
    pub pool_idle: u32,
}

/// Rows per table, installation-wide. Counts only.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct EntityCounts {
    pub nodes: i64,
    pub edges: i64,
    pub tags: i64,
    pub notes: i64,
    pub tasks: i64,
    pub attachments: i64,
    pub favorites: i64,
}

/// The `GET /api/metrics/db` answer.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct DbStatements {
    /// `false` when the server doesn't preload `pg_stat_statements` or the
    /// extension isn't created; `statements` is then empty.
    pub enabled: bool,
    /// The largest total time first, at most [`TOP_STATEMENTS`].
    pub statements: Vec<DbStatement>,
}

/// One normalised statement's totals since the statistics were last reset.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct DbStatement {
    /// Whitespace collapsed, at most [`STATEMENT_CHARS`] characters.
    pub query: String,
    pub calls: i64,
    pub total_ms: f64,
    pub mean_ms: f64,
    pub max_ms: f64,
    pub rows: i64,
    /// Blocks read from disk rather than the buffer cache.
    pub blks_read: i64,
}

/// How many statements `GET /api/metrics/db` lists.
pub const TOP_STATEMENTS: i64 = 15;

/// How much of a statement's text is shown.
pub const STATEMENT_CHARS: usize = 400;

/// A statement without its SQL comments (`-- …` to the end of the line, and
/// `/* … */`, which nest in Postgres). `pg_stat_statements` replaces values
/// with `$1` but keeps the first-seen text otherwise, comments included, so
/// a comment typed in psql (`-- for ann@example.com`) would reach the page.
/// Quoted text (`'…'`, `"…"`) is copied as it is, so a `--` inside a quoted
/// identifier isn't taken for a comment. An unterminated comment drops the
/// rest. Each comment becomes one space, so the words around it stay apart.
pub fn strip_sql_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' => {
                // Copy up to the closing quote; a doubled quote is an escape
                // and simply continues the run.
                out.push(c);
                for q in chars.by_ref() {
                    out.push(q);
                    if q == c {
                        break;
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                out.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut depth = 1u32;
                while depth > 0 {
                    match chars.next() {
                        None => break,
                        Some('/') if chars.peek() == Some(&'*') => {
                            chars.next();
                            depth += 1;
                        }
                        Some('*') if chars.peek() == Some(&'/') => {
                            chars.next();
                            depth -= 1;
                        }
                        Some(_) => {}
                    }
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// A statement for display: runs of whitespace (newlines, indents) become
/// one space, and text past `max_chars` characters becomes `…`.
pub fn compact_statement(query: &str, max_chars: usize) -> String {
    let collapsed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(max_chars).collect();
    cut.push('…');
    cut
}

/// How a latency reads at a glance. Warm colours on the page are for these
/// only (the heat tokens); everything else stays neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Latency {
    Fine,
    /// At least [`SLOW_MS`].
    Slow,
    /// At least [`VERY_SLOW_MS`].
    VerySlow,
}

pub const SLOW_MS: f64 = 250.0;
pub const VERY_SLOW_MS: f64 = 1000.0;

impl Latency {
    pub fn of(ms: f64) -> Self {
        if ms >= VERY_SLOW_MS {
            Self::VerySlow
        } else if ms >= SLOW_MS {
            Self::Slow
        } else {
            Self::Fine
        }
    }
}

/// The column the route table is sorted by. Each sorts the largest first,
/// except [`RouteSort::Route`], which is alphabetical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RouteSort {
    #[default]
    Total,
    Count,
    P95,
    Max,
    Route,
}

/// Sort routes by `key`; ties fall back to the route name, so the order is
/// stable from one refresh to the next.
pub fn sort_routes(routes: &mut [RouteLatency], key: RouteSort) {
    let num = |r: &RouteLatency| match key {
        RouteSort::Total => r.total_ms,
        RouteSort::Count => r.count as f64,
        RouteSort::P95 => r.p95_ms,
        RouteSort::Max => r.max_ms,
        RouteSort::Route => 0.0,
    };
    routes.sort_by(|a, b| {
        let by_key = match key {
            RouteSort::Route => std::cmp::Ordering::Equal,
            _ => num(b).total_cmp(&num(a)),
        };
        by_key.then_with(|| a.route.cmp(&b.route))
    });
}

/// Uptime as the page shows it: `45 s`, `12 min`, `3 h 20 min`, `2 d 4 h`.
pub fn format_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{secs} s")
    }
}

/// A duration in ms as the page shows it: `0.4 ms`, `38 ms`, `1.2 s`.
pub fn format_ms(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.1} s", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{ms:.1} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(name: &str, count: u64, total: f64, p95: f64, max: f64) -> RouteLatency {
        RouteLatency {
            route: name.into(),
            count,
            total_ms: total,
            mean_ms: total / count as f64,
            p50_ms: 0.0,
            p95_ms: p95,
            p99_ms: 0.0,
            max_ms: max,
        }
    }

    fn names(routes: &[RouteLatency]) -> Vec<&str> {
        routes.iter().map(|r| r.route.as_str()).collect()
    }

    #[test]
    fn routes_sort_by_each_column_largest_first() {
        let mut rs = vec![
            route("GET /b", 10, 50.0, 20.0, 30.0),
            route("GET /a", 2, 900.0, 600.0, 800.0),
            route("GET /c", 99, 100.0, 2.0, 1500.0),
        ];
        sort_routes(&mut rs, RouteSort::Total);
        assert_eq!(names(&rs), ["GET /a", "GET /c", "GET /b"]);
        sort_routes(&mut rs, RouteSort::Count);
        assert_eq!(names(&rs), ["GET /c", "GET /b", "GET /a"]);
        sort_routes(&mut rs, RouteSort::P95);
        assert_eq!(names(&rs), ["GET /a", "GET /b", "GET /c"]);
        sort_routes(&mut rs, RouteSort::Max);
        assert_eq!(names(&rs), ["GET /c", "GET /a", "GET /b"]);
        sort_routes(&mut rs, RouteSort::Route);
        assert_eq!(names(&rs), ["GET /a", "GET /b", "GET /c"]);
    }

    #[test]
    fn ties_fall_back_to_the_route_name() {
        let mut rs = vec![
            route("GET /z", 1, 5.0, 1.0, 1.0),
            route("GET /m", 1, 5.0, 1.0, 1.0),
        ];
        sort_routes(&mut rs, RouteSort::Total);
        assert_eq!(names(&rs), ["GET /m", "GET /z"]);
    }

    #[test]
    fn latency_levels_start_at_their_thresholds() {
        assert_eq!(Latency::of(0.0), Latency::Fine);
        assert_eq!(Latency::of(249.9), Latency::Fine);
        assert_eq!(Latency::of(SLOW_MS), Latency::Slow);
        assert_eq!(Latency::of(999.9), Latency::Slow);
        assert_eq!(Latency::of(VERY_SLOW_MS), Latency::VerySlow);
    }

    #[test]
    fn uptime_reads_in_its_largest_units() {
        assert_eq!(format_uptime(0), "0 s");
        assert_eq!(format_uptime(59), "59 s");
        assert_eq!(format_uptime(60), "1 min");
        assert_eq!(format_uptime(3_599), "59 min");
        assert_eq!(format_uptime(3_600 * 3 + 60 * 20 + 5), "3 h 20 min");
        assert_eq!(format_uptime(86_400 * 2 + 3_600 * 4 + 59), "2 d 4 h");
    }

    #[test]
    fn durations_keep_useful_precision() {
        assert_eq!(format_ms(0.4), "0.4 ms");
        assert_eq!(format_ms(9.96), "10.0 ms");
        assert_eq!(format_ms(38.4), "38 ms");
        assert_eq!(format_ms(999.0), "999 ms");
        assert_eq!(format_ms(1234.0), "1.2 s");
    }

    #[test]
    fn statements_are_collapsed_and_cut_by_characters() {
        assert_eq!(
            compact_statement("SELECT a\n    FROM t\n\tWHERE id = $1  ", 100),
            "SELECT a FROM t WHERE id = $1"
        );
        assert_eq!(compact_statement("SELECT 1", 8), "SELECT 1");
        assert_eq!(compact_statement("SELECT 12", 8), "SELECT 1…");
        // Multibyte text is cut on a character, never mid-byte.
        assert_eq!(compact_statement("SELECT 'ééé'", 9), "SELECT 'é…");
    }

    #[test]
    fn comments_are_removed_wherever_they_sit() {
        let c = |s: &str| compact_statement(&strip_sql_comments(s), 200);
        assert_eq!(
            c("select * from t where a = $1 /* leak@example.com */"),
            "select * from t where a = $1"
        );
        assert_eq!(
            c("select a -- for ann@example.com\nfrom t"),
            "select a from t"
        );
        assert_eq!(c("select 1 -- trailing, no newline"), "select 1");
        // Postgres block comments nest.
        assert_eq!(c("select /* a /* b */ still a */ 1"), "select 1");
        // Unterminated: the rest is dropped, never shown.
        assert_eq!(c("select 1 /* never closed secret"), "select 1");
        assert_eq!(c("select 1/**/from t"), "select 1 from t");
    }

    #[test]
    fn quoted_text_is_not_a_comment() {
        assert_eq!(
            strip_sql_comments(r#"select "a--b", "c/*d" from t -- x"#),
            r#"select "a--b", "c/*d" from t  "#
        );
        assert_eq!(
            strip_sql_comments("select 'it''s -- not' -- yes"),
            "select 'it''s -- not'  "
        );
        // A quote inside a comment doesn't open a string.
        assert_eq!(
            strip_sql_comments("select 1 -- don't\nfrom t"),
            "select 1  from t"
        );
    }

    #[test]
    fn a_minus_or_slash_alone_is_kept() {
        assert_eq!(strip_sql_comments("select a - b / c"), "select a - b / c");
    }

    #[test]
    fn a_route_from_before_total_ms_still_parses() {
        let r: RouteLatency = serde_json::from_str(
            r#"{"route":"GET /x","count":1,"mean_ms":1.0,"p50_ms":1.0,"p95_ms":1.0,"p99_ms":1.0,"max_ms":1.0}"#,
        )
        .unwrap();
        assert_eq!(r.total_ms, 0.0);
    }

    #[test]
    fn the_snapshot_round_trips() {
        let m = Metrics {
            version: "3.9.0".into(),
            uptime_secs: 5,
            db: DbMetrics {
                pool_size: 3,
                pool_idle: 2,
            },
            counts: EntityCounts {
                nodes: 1,
                edges: 2,
                tags: 3,
                notes: 4,
                tasks: 5,
                attachments: 6,
                favorites: 7,
            },
            requests: RequestCounts {
                total: 10,
                status_2xx: 9,
                status_3xx: 0,
                status_4xx: 1,
                status_5xx: 0,
            },
            routes: vec![route("GET /api/nodes/{id}", 2, 4.0, 3.0, 3.0)],
        };
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Metrics>(&json).unwrap(), m);
    }
}
