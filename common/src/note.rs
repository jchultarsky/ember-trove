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
    /// When the note was written: its place in the chronology (§9.2).
    /// Never moved, by an edit or by anything else.
    pub created_at: DateTime<Utc>,
    /// When the row was last written, for any reason (an edit, a colour,
    /// a trip through the trash). Nothing a person reads: "edited" is
    /// [`Self::edited_at`].
    pub updated_at: DateTime<Utc>,
    /// When the note's words were last changed, if they ever were (S6d;
    /// §9.2: "the note shows that it was edited"). Only a change to the
    /// text sets it: not a colour, not a delete and restore.
    #[serde(default)]
    pub edited_at: Option<DateTime<Utc>>,
}

/// A note as one reader may act on it (S6d): the note, and whether this
/// reader may edit it and remove it. The flags are the write routes' own
/// rules, read for the reader (`repo::note::author_may_write`,
/// `may_remove`), so a button is offered only where the call would pass.
/// They say nothing about anyone else.
///
/// A wrapper, not fields of [`Note`]: a note is also what a backup reads
/// and what the write routes answer with, where there is no reader.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReaderNote {
    #[serde(flatten)]
    pub note: Note,
    /// The reader wrote it and may still edit its node.
    #[serde(default)]
    pub may_edit: bool,
    /// That, or the reader owns the node, or is an admin.
    #[serde(default)]
    pub may_remove: bool,
}

impl std::ops::Deref for ReaderNote {
    type Target = Note;
    fn deref(&self) -> &Note {
        &self.note
    }
}

/// Note enriched with the parent node's title — used in the central feed.
/// `node_title` is `None` only if the node can't be read back.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FeedNote {
    #[serde(flatten)]
    pub note: ReaderNote,
    pub node_title: Option<String>,
}

/// Whether a note written before `edited_at` existed (migration 052)
/// counts as edited, and since when: the mark its page showed then, which
/// was "its row was last written three whole seconds or more after it was
/// created" (owner decision, 2026-11-14: existing marks are kept).
///
/// Some of those marks are wrong (a restore from the trash or a colour
/// change moved `updated_at` too); what really happened is not
/// recoverable. Migration 052 applies this rule in SQL, and a Postgres
/// test holds the two together; restoring an older archive applies it
/// here ([`restored_edited_at`]).
#[must_use]
pub fn legacy_edited_at(
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    ((updated_at - created_at).num_seconds() > 2).then_some(updated_at)
}

/// A restored note's `edited_at`, from what its archive says:
/// - nothing (`None`): an archive from before the field (formats 3 to
///   10), so the legacy rule decides;
/// - null (`Some(None)`): never edited;
/// - a date: that date.
#[must_use]
pub fn restored_edited_at(
    archived: Option<Option<DateTime<Utc>>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match archived {
        Some(known) => known,
        None => legacy_edited_at(created_at, updated_at),
    }
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
    /// Most recently edited first: by the date a note's words last
    /// changed, or the date it was written if they never did. The wire
    /// value stays `updated`.
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
        .map(plain_line)
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| UNTITLED_NOTE.to_string())
}

/// One line of Markdown as the text a reader sees: block marks (heading
/// `#`s, `>`, list markers) and inline marks gone, links reduced to what
/// they show. The one stripper for a note's title and a body's plain
/// preview (#281, 3.4).
pub(crate) fn plain_line(line: &str) -> String {
    clean_inline(strip_block_prefix(line))
}

/// Code-fence delimiters (```` ``` ````, `~~~`) and thematic breaks (`---`,
/// `***`, `___`, spaced variants) carry no title text.
pub(crate) fn is_fence_or_rule(line: &str) -> bool {
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
    // Wiki links first, by the one scanner (#281, 3.1).
    let unlinked = crate::wikilink::replace_wikilinks(line, |link| link.display.to_string());
    let mut out = String::with_capacity(unlinked.len());
    let mut rest = unlinked.as_str();
    while !rest.is_empty() {
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
mod edited_tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn at(secs: i64, millis: i64) -> DateTime<Utc> {
        let base = Utc
            .with_ymd_and_hms(2026, 3, 1, 9, 0, 0)
            .single()
            .expect("a date");
        base + Duration::seconds(secs) + Duration::milliseconds(millis)
    }

    /// The old page showed "edited" when the whole seconds between the two
    /// dates were more than 2: from 3.000 s, not from 2.001 s.
    #[test]
    fn a_legacy_note_counts_as_edited_from_three_whole_seconds() {
        let created = at(0, 0);
        assert_eq!(legacy_edited_at(created, created), None);
        assert_eq!(legacy_edited_at(created, at(2, 0)), None);
        assert_eq!(legacy_edited_at(created, at(2, 999)), None);
        assert_eq!(legacy_edited_at(created, at(3, 0)), Some(at(3, 0)));
        assert_eq!(legacy_edited_at(created, at(3600, 0)), Some(at(3600, 0)));
        // A clock that ran backwards marks nothing.
        assert_eq!(legacy_edited_at(at(10, 0), created), None);
    }

    #[test]
    fn a_restored_note_takes_what_its_archive_says_about_edits() {
        let (created, later) = (at(0, 0), at(60, 0));
        // An archive from before the field: the legacy rule.
        assert_eq!(restored_edited_at(None, created, later), Some(later));
        assert_eq!(restored_edited_at(None, created, created), None);
        // An archive that says "never edited" is believed, whatever
        // `updated_at` did (a recolour moved it).
        assert_eq!(restored_edited_at(Some(None), created, later), None);
        // And one that names a date keeps it.
        let edited = at(30, 0);
        assert_eq!(
            restored_edited_at(Some(Some(edited)), created, later),
            Some(edited)
        );
    }

    #[test]
    fn a_readers_note_is_the_note_with_two_flags_beside_it() {
        let note = Note {
            id: NoteId(uuid::Uuid::from_u128(1)),
            node_id: NodeId(uuid::Uuid::from_u128(2)),
            owner_id: "alice".into(),
            body: "hello".into(),
            color: "default".into(),
            created_at: at(0, 0),
            updated_at: at(0, 0),
            edited_at: None,
        };
        let reader = ReaderNote {
            note: note.clone(),
            may_edit: true,
            may_remove: false,
        };
        let json = serde_json::to_value(&reader).expect("json");
        assert_eq!(
            json["body"], "hello",
            "flat: an older page reads it as a note"
        );
        assert_eq!(json["may_edit"], true);
        assert_eq!(json["may_remove"], false);
        assert_eq!(reader.body, "hello", "and it derefs to the note");
        // A note without the new fields (an answer cached by an older
        // release) reads as unedited, with nothing offered.
        let old = serde_json::json!({
            "id": note.id, "node_id": note.node_id, "owner_id": "alice",
            "body": "hello", "created_at": note.created_at,
            "updated_at": note.updated_at,
        });
        let read: ReaderNote = serde_json::from_value(old).expect("an older answer");
        assert_eq!(read.edited_at, None);
        assert!(!read.may_edit && !read.may_remove);
    }
}

#[cfg(test)]
mod tests {
    use super::{UNTITLED_NOTE, note_title};

    #[test]
    fn a_wiki_link_reads_as_what_it_shows() {
        // One scanner for every reader (#281, 3.1): the shown part is
        // trimmed here too, and an empty one falls back to the title.
        assert_eq!(
            note_title("Ask [[Alice Smith| Alice ]] today"),
            "Ask Alice today"
        );
        assert_eq!(note_title("About [[Project X|]]"), "About Project X");
        assert_eq!(note_title("[[Plain]] link"), "Plain link");
    }

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
