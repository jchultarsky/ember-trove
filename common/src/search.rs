use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, SearchPresetId};
use crate::node::{NodeStatus, NodeType};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchQuery {
    pub q: String,
    pub fuzzy: Option<bool>,
    pub node_type: Option<NodeType>,
    /// When set, only nodes with this status are returned.
    pub status: Option<NodeStatus>,
    /// Comma-separated tag UUIDs. All listed tags are applied as a filter.
    pub tag_ids: Option<String>,
    /// How to combine multiple tags: `"or"` (default) or `"and"`.
    pub tag_op: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    /// Sort order: `"relevance"` (default for text search), `"updated_desc"`,
    /// `"updated_asc"`, `"title_asc"`, `"title_desc"`.
    pub sort: Option<String>,
    /// Restrict results to nodes updated on or after this date (`YYYY-MM-DD`).
    pub updated_after: Option<NaiveDate>,
    /// Restrict results to nodes updated on or before this date (`YYYY-MM-DD`).
    pub updated_before: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResult {
    pub node_id: NodeId,
    pub title: String,
    pub slug: String,
    pub snippet: Option<String>,
    pub rank: f32,
    /// Lowercase node type string, e.g. `"article"`, `"project"`.
    pub node_type: String,
    /// Lowercase status string, e.g. `"draft"`, `"published"`, `"archived"`.
    pub status: String,
    /// Where the match was found: `"node"` (title/body), `"note"`, or `"task"`.
    /// `None` for browse (empty-query) results.
    pub match_source: Option<String>,
    /// The node title with query matches wrapped in `<mark>…</mark>`
    /// (`ts_headline` with `HighlightAll`, so never shortened), for node-body
    /// full-text matches. `None` in browse and fuzzy mode and for note/task
    /// matches. Render it only if [`title_highlight_is_full`] holds.
    pub highlighted_title: Option<String>,
    /// When the node was last updated (shown on result cards). Optional for
    /// wire compatibility with older servers.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

const MARK_OPEN: &str = "<mark>";
const MARK_CLOSE: &str = "</mark>";

/// `true` when `highlighted` is exactly `title` plus `<mark>` tags: the
/// guard that keeps a shortened headline fragment from ever replacing the
/// real title in a results list (#107).
pub fn title_highlight_is_full(highlighted: &str, title: &str) -> bool {
    highlighted.replace(MARK_OPEN, "").replace(MARK_CLOSE, "") == title
}

/// `true` when a server-highlighted string carries at least one match.
/// For a node result this says whether the query hit the title ("in
/// title") or only the body ("in text"); for a snippet, whether it shows a
/// match at all or is just the body's leading words.
pub fn has_mark(highlighted: &str) -> bool {
    highlighted.contains(MARK_OPEN)
}

/// Split `text` into `(segment, is_match)` runs, marking case-insensitive
/// literal occurrences of the query's words (two characters or more).
///
/// Client-side highlighting for results the server didn't mark (fuzzy mode,
/// note/task matches), so toggling Fuzzy doesn't change how results are
/// annotated. Rendered as escaped text plus `<mark>` elements — never as
/// HTML — so it can't open an injection path.
pub fn highlight_segments(text: &str, query: &str) -> Vec<(String, bool)> {
    let terms: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|w| w.chars().flat_map(char::to_lowercase).collect::<Vec<_>>())
        .filter(|t| t.len() >= 2)
        .collect();
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<Option<char>> = chars
        .iter()
        .map(|c| {
            // Only 1:1 lowercase mappings are matchable; others never match,
            // which keeps char positions aligned with the original text.
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => Some(x),
                _ => None,
            }
        })
        .collect();

    let mut out: Vec<(String, bool)> = Vec::new();
    let mut push = |seg: String, hit: bool| {
        if seg.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((prev, h)) if *h == hit => prev.push_str(&seg),
            _ => out.push((seg, hit)),
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let hit_len = terms
            .iter()
            .filter(|t| {
                i + t.len() <= chars.len()
                    && t.iter()
                        .enumerate()
                        .all(|(k, tc)| lower[i + k] == Some(*tc))
            })
            .map(Vec::len)
            .max();
        match hit_len {
            Some(n) => {
                push(chars[i..i + n].iter().collect(), true);
                i += n;
            }
            None => {
                push(chars[i].to_string(), false);
                i += 1;
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
}

// ── Search presets ─────────────────────────────────────────────────────────────

/// A saved combination of search query + filter settings.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchPreset {
    pub id: SearchPresetId,
    pub owner_id: String,
    pub name: String,
    /// The text search query (may be empty).
    pub query: String,
    pub fuzzy: bool,
    pub published_only: bool,
    /// Comma-separated tag UUID strings (mirrors `SearchQuery::tag_ids`).
    pub tag_ids: String,
    /// `"or"` (default) or `"and"`.
    pub tag_op: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, garde::Validate)]
pub struct CreateSearchPresetRequest {
    #[garde(length(min = 1, max = 100))]
    pub name: String,
    #[garde(length(max = 1000))]
    pub query: String,
    #[garde(skip)]
    pub fuzzy: bool,
    #[garde(skip)]
    pub published_only: bool,
    #[garde(length(max = 2000))]
    pub tag_ids: String,
    #[garde(length(max = 8))]
    pub tag_op: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_title_highlight_passes_the_guard() {
        assert!(title_highlight_is_full(
            "Buy a gift for <mark>Sam</mark>",
            "Buy a gift for Sam"
        ));
        // No match in the title (body-only hit): still the full title.
        assert!(title_highlight_is_full(
            "Master Bath Remodel",
            "Master Bath Remodel"
        ));
    }

    #[test]
    fn shortened_headline_fragments_fail_the_guard() {
        // The #107 defect: ts_headline returned fragments of the title.
        assert!(!title_highlight_is_full(
            "Gift for <mark>Sam</mark>",
            "Buy a gift for Sam"
        ));
        assert!(!title_highlight_is_full("Master", "Master Bath Remodel"));
    }

    #[test]
    fn has_mark_detects_matches() {
        assert!(has_mark("Buy a gift for <mark>Sam</mark>"));
        assert!(!has_mark("Master Bath Remodel"));
    }

    fn render(segs: &[(String, bool)]) -> String {
        segs.iter()
            .map(|(s, hit)| if *hit { format!("[{s}]") } else { s.clone() })
            .collect()
    }

    #[test]
    fn highlight_segments_marks_case_insensitive_words() {
        let segs = highlight_segments("Buy a Gift for Sam", "gift sam");
        assert_eq!(render(&segs), "Buy a [Gift] for [Sam]");
        // Every character is preserved, in order.
        let joined: String = segs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "Buy a Gift for Sam");
    }

    #[test]
    fn highlight_segments_ignores_short_terms_and_empty_queries() {
        assert_eq!(render(&highlight_segments("a b c", "a")), "a b c");
        assert_eq!(render(&highlight_segments("Anything", "")), "Anything");
        assert_eq!(highlight_segments("", "gift"), Vec::<(String, bool)>::new());
    }

    #[test]
    fn highlight_segments_prefers_the_longest_term_and_handles_unicode() {
        assert_eq!(
            render(&highlight_segments("Remodeling", "rem remodel")),
            "[Remodel]ing"
        );
        assert_eq!(
            render(&highlight_segments("Café crème", "CRÈME")),
            "Café [crème]"
        );
        // Text with characters whose lowercase is not 1:1 stays aligned.
        assert_eq!(
            render(&highlight_segments("İstanbul trip", "trip")),
            "İstanbul [trip]"
        );
    }
}
