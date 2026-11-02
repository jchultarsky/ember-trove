use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, NoteId};

fn default_note_color() -> String {
    "default".to_string()
}

/// A note on a node (ONTOLOGY §9.2). Every note has one: input that
/// belongs nowhere yet is a capture (`crate::capture`, #234).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Note {
    pub id: NoteId,
    pub node_id: NodeId,
    pub owner_id: String,
    pub body: String,
    /// Named palette key for the card background colour (e.g. "amber", "rose").
    /// "default" means neutral / no colour.
    #[serde(default = "default_note_color")]
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Note enriched with the parent node's title — used in the central feed.
/// `node_title` is `None` only if the node can't be read back.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FeedNote {
    #[serde(flatten)]
    pub note: Note,
    pub node_title: Option<String>,
}

/// Request body for creating a new note.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateNoteRequest {
    #[garde(length(min = 1, max = 10000))]
    pub body: String,
    /// Palette key: "default" | "amber" | "rose" | "lime" | "sky" | "violet".
    #[garde(length(min = 1, max = 20))]
    #[serde(default = "default_note_color")]
    pub color: String,
}

/// `POST /notes`: a note on a node, chosen in the request. A note always
/// has a node (§9.2); quick thoughts go to a capture (#234).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateFeedNoteRequest {
    #[garde(dive)]
    #[serde(flatten)]
    pub note: CreateNoteRequest,
    #[garde(skip)]
    pub node_id: NodeId,
}

/// Sort order for the notes feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum NoteSort {
    /// Newest created first (default).
    #[default]
    Newest,
    /// Oldest created first.
    Oldest,
    /// Most recently updated first.
    Updated,
}

impl NoteSort {
    /// Parse the `sort` query value; anything unrecognized falls back to `Newest`.
    #[must_use]
    pub fn from_param(s: Option<&str>) -> Self {
        match s {
            Some("oldest") => Self::Oldest,
            Some("updated") => Self::Updated,
            _ => Self::Newest,
        }
    }
}

/// Query parameters for the notes feed (filter + sort); all optional.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct NoteFeedParams {
    /// Filter to a specific node (UUID string).
    pub node_id: Option<String>,
    /// Inclusive lower bound on `created_at` (`YYYY-MM-DD`).
    pub from: Option<String>,
    /// Inclusive upper bound on `created_at` (`YYYY-MM-DD`).
    pub to: Option<String>,
    /// Case-insensitive substring filter on the note body.
    pub q: Option<String>,
    /// Sort order: `newest` (default) | `oldest` | `updated`.
    pub sort: Option<String>,
    /// 1-based page number (default 1).
    pub page: Option<u32>,
    /// Rows per page (default 200, server-clamped to 1000).
    pub per_page: Option<u32>,
}

/// Request body for editing an existing note.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateNoteRequest {
    #[garde(length(min = 1, max = 10000))]
    pub body: String,
    /// Palette key for the note background.
    #[garde(length(min = 1, max = 20))]
    #[serde(default = "default_note_color")]
    pub color: String,
}

/// Shown when a note has no text to use as its collapsed title.
pub const UNTITLED_NOTE: &str = "Untitled note";

/// The one-line title shown for a collapsed note: its first non-empty line
/// with Markdown syntax stripped (`## Meeting **notes**` → `Meeting notes`).
/// Truncation is left to CSS so it adapts to the available width.
#[must_use]
pub fn note_title(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !is_fence_or_rule(line))
        .map(|line| clean_inline(strip_block_prefix(line)))
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| UNTITLED_NOTE.to_string())
}

/// Code-fence delimiters (```` ``` ````, `~~~`) and thematic breaks (`---`,
/// `***`, `___`, spaced variants) carry no title text.
fn is_fence_or_rule(line: &str) -> bool {
    if line.starts_with("```") || line.starts_with("~~~") {
        return true;
    }
    let marks: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3
        && marks
            .first()
            .is_some_and(|&m| matches!(m, '-' | '*' | '_') && marks.iter().all(|&c| c == m))
}

/// Strip leading block syntax — headings, quotes, list and task markers —
/// repeatedly, so nested forms like `> - [ ] item` reduce to `item`.
fn strip_block_prefix(mut line: &str) -> &str {
    loop {
        let before = line;
        line = line.trim_start();
        if let Some(rest) = line.strip_prefix('#') {
            let rest = rest.trim_start_matches('#');
            // `#tag` is not a heading — only `#`s followed by a space (or nothing).
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                line = rest;
            }
        }
        if let Some(rest) = line.strip_prefix('>') {
            line = rest;
        }
        for marker in ["- ", "* ", "+ "] {
            if let Some(rest) = line.strip_prefix(marker) {
                line = rest;
            }
        }
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            // ASCII digits are one byte each, so `digits` is a char boundary.
            let rest = line.get(digits..).unwrap_or_default();
            if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                line = rest;
            }
        }
        for task in ["[ ] ", "[x] ", "[X] "] {
            if let Some(rest) = line.strip_prefix(task) {
                line = rest;
            }
        }
        if line == before {
            return line.trim();
        }
    }
}

/// Reduce inline Markdown to its visible text: links/images/wikilinks keep
/// their label, emphasis and code marks go, whitespace collapses.
fn clean_inline(line: &str) -> String {
    let unlinked = strip_links(line);
    let mut text = unlinked;
    for mark in ["**", "__", "~~", "`"] {
        text = text.replace(mark, "");
    }
    let text = strip_lone_emphasis(&text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `[[target|display]]` → display, `[[target]]` → target,
/// `![alt](url)` → alt, `[text](url)` → text. Anything else is kept verbatim.
fn strip_links(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(end) = after.find("]]")
        {
            let inner = after.get(..end).unwrap_or_default();
            let shown = inner.split_once('|').map_or(inner, |(_, display)| display);
            out.push_str(shown);
            rest = after.get(end + 2..).unwrap_or_default();
            continue;
        }
        // Images and links both keep their bracketed text.
        let after_open = rest.strip_prefix("![").or_else(|| rest.strip_prefix('['));
        if let Some(after) = after_open
            && let Some(close) = after.find("](")
            && let Some(url_len) = after.get(close + 2..).and_then(|u| u.find(')'))
        {
            out.push_str(after.get(..close).unwrap_or_default());
            rest = after.get(close + 2 + url_len + 1..).unwrap_or_default();
            continue;
        }
        let mut chars = rest.chars();
        if let Some(c) = chars.next() {
            out.push(c);
        }
        rest = chars.as_str();
    }
    out
}

/// Remove single `*` / `_` used as emphasis — i.e. touching a word boundary on
/// one side — while keeping them inside words (`snake_case`, `2*3`).
fn strip_lone_emphasis(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let is_word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
    chars
        .iter()
        .enumerate()
        .filter(|&(i, &c)| {
            if c != '*' && c != '_' {
                return true;
            }
            let prev = i.checked_sub(1).and_then(|p| chars.get(p));
            let next = chars.get(i + 1);
            // Keep only when flanked by word characters on both sides.
            is_word(prev) && is_word(next)
        })
        .map(|(_, &c)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{UNTITLED_NOTE, note_title};

    #[test]
    fn uses_the_first_non_empty_line() {
        assert_eq!(note_title("\n\n  Groceries  \n- milk\n- eggs"), "Groceries");
    }

    #[test]
    fn strips_heading_and_emphasis_marks() {
        assert_eq!(note_title("## Meeting **notes**"), "Meeting notes");
        assert_eq!(
            note_title("# _Ideas_ for ~~v2~~ `graph`"),
            "Ideas for v2 graph"
        );
        assert_eq!(note_title("### __Bold__ and *em*"), "Bold and em");
    }

    #[test]
    fn strips_list_quote_and_task_markers() {
        assert_eq!(note_title("- first item"), "first item");
        assert_eq!(note_title("* starred item"), "starred item");
        assert_eq!(note_title("+ plus item"), "plus item");
        assert_eq!(note_title("1. numbered"), "numbered");
        assert_eq!(note_title("> quoted line"), "quoted line");
        assert_eq!(note_title("- [ ] todo item"), "todo item");
        assert_eq!(note_title("- [x] done item"), "done item");
    }

    #[test]
    fn links_images_and_wikilinks_keep_their_text() {
        assert_eq!(
            note_title("See [the docs](https://x.example/a)"),
            "See the docs"
        );
        assert_eq!(
            note_title("![diagram](img.png) overview"),
            "diagram overview"
        );
        assert_eq!(note_title("Ref [[Graph Layout]]"), "Ref Graph Layout");
        assert_eq!(note_title("Ref [[Graph Layout|layout]]"), "Ref layout");
    }

    #[test]
    fn keeps_underscores_and_asterisks_inside_words() {
        assert_eq!(
            note_title("rename snake_case_name"),
            "rename snake_case_name"
        );
        assert_eq!(note_title("2*3 = 6"), "2*3 = 6");
    }

    #[test]
    fn skips_fences_and_rules_before_real_text() {
        assert_eq!(note_title("```\nlet x = 1;\n```"), "let x = 1;");
        assert_eq!(note_title("---\nAfter the rule"), "After the rule");
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(note_title("  lots   of\tspace  "), "lots of space");
    }

    #[test]
    fn empty_or_markup_only_body_gets_a_placeholder() {
        assert_eq!(note_title(""), UNTITLED_NOTE);
        assert_eq!(note_title("   \n\n"), UNTITLED_NOTE);
        assert_eq!(note_title("## \n**  **"), UNTITLED_NOTE);
    }
}
