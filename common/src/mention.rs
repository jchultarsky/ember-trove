//! Mentions of People in text (S9, #360; ONTOLOGY §7.3 and §14).
//!
//! A mention is written `@[Ada Lovelace](person:<id>)`. **The id is the
//! identity.** The label is what the author saw when they picked the
//! person: it keeps the raw text readable, and it is what shows when the
//! mention can't be resolved. It is never used to find anyone.
//!
//! This module is the one reader of that syntax, as [`crate::wikilink`]
//! is for `[[Title]]`: the API's sync, the renderer, the picker and the
//! plain-text views all go through [`mentions`]. Who may make a mention,
//! and to whom a person is named, are the API's rules
//! (`api/src/repo/mention.rs`); nothing here decides that.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::id::{NodeId, NoteId, PersonId};
use crate::node::NodeType;
use crate::wikilink::{code_ranges, covered, wikilinks};

/// What opens a mention, and what sits between its label and its id.
const OPEN: &str = "@[";
const MIDDLE: &str = "](person:";
/// A UUID as this app writes one: 36 characters, lower case, hyphenated.
const ID_LEN: usize = 36;

/// One `@[label](person:id)` in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention<'a> {
    /// Byte range of the whole token in the source, `@` included.
    pub range: Range<usize>,
    pub person_id: PersonId,
    /// The name as it was written, trimmed. Never empty.
    pub label: &'a str,
}

/// Every mention in `text`, in order. The one scanner.
///
/// - `@[label](person:id)`, on one line.
/// - The label is not empty and holds no `[`, `]` or backtick
///   ([`mention_token`] never writes one).
/// - The id has one spelling: 36 characters, lower case, hyphenated.
///   An upper-case or braced UUID is plain text. One spelling means two
///   tokens for one person can't look different to the sync and the
///   same to the renderer.
/// - Nothing inside code is a mention, and nothing inside a wiki link
///   (`[[a|@[b](person:…)]]` is the wiki link's display text).
#[must_use]
pub fn mentions(text: &str) -> Vec<Mention<'_>> {
    let mut off_limits = code_ranges(text);
    off_limits.extend(wikilinks(text).into_iter().map(|link| link.range));
    // A binary search, not a look through every range for every `@[`:
    // this runs on every save, on up to a megabyte.
    let skipped = covered(&off_limits);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text.get(from..).and_then(|rest| rest.find(OPEN)) {
        let open = from + rel;
        let label_start = open + OPEN.len();
        from = label_start;
        if skipped(open) {
            continue;
        }
        let Some(token) = token_at(text, open) else {
            continue;
        };
        from = token.range.end;
        found.push(token);
    }
    found
}

/// The mention that starts at byte `open` (an `@[`), if it is one.
fn token_at(text: &str, open: usize) -> Option<Mention<'_>> {
    let label_start = open + OPEN.len();
    let rest = text.get(label_start..)?;
    // The label ends at the first `]`. A `[`, a backtick or a line break
    // before it means this is not a mention, and the look stops there:
    // never to the end of a long line for each `@[` on it.
    let label_len = rest.find([']', '[', '`', '\n'])?;
    if !rest.get(label_len..)?.starts_with(']') {
        return None;
    }
    let raw_label = rest.get(..label_len)?;
    let label = raw_label.trim();
    if label.is_empty() {
        return None;
    }
    let after_label = rest.get(label_len..)?;
    let id_and_more = after_label.strip_prefix(MIDDLE)?;
    let id = id_and_more.get(..ID_LEN)?;
    if !id_and_more.get(ID_LEN..)?.starts_with(')') {
        return None;
    }
    let person_id = canonical_id(id)?;
    let end = label_start + label_len + MIDDLE.len() + ID_LEN + 1;
    Some(Mention {
        range: open..end,
        person_id,
        label,
    })
}

/// `id` as a person id, if it is spelled the one way this app spells it.
fn canonical_id(id: &str) -> Option<PersonId> {
    let parsed = Uuid::parse_str(id).ok()?;
    (parsed.hyphenated().to_string() == id).then_some(PersonId(parsed))
}

/// `text` with each mention replaced by what `shown` returns for it.
/// Everything else is copied as it is, code included.
pub fn replace_mentions(text: &str, mut shown: impl FnMut(&Mention<'_>) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for mention in mentions(text) {
        out.push_str(text.get(at..mention.range.start).unwrap_or_default());
        out.push_str(&shown(&mention));
        at = mention.range.end;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    out
}

/// The people `text` mentions: each once, in order of first mention.
#[must_use]
pub fn mentioned_ids(text: &str) -> Vec<PersonId> {
    let mut ids = Vec::new();
    for mention in mentions(text) {
        if !ids.contains(&mention.person_id) {
            ids.push(mention.person_id);
        }
    }
    ids
}

/// The people `after` mentions **more often** than `before` did: the
/// mentions a save added, when `before` is the text as it was and
/// `after` the text being saved.
///
/// SECURITY: this is what "the saver made the mention" means. A token
/// that was in the text already was put there by an earlier save,
/// perhaps someone else's: a co-editor can plant a token with the id of
/// another account's person (hidden in a comment, say), and that
/// person's owner saving the body afterwards must not make it real.
/// Counted, not just "new": the owner mentioning that person themself,
/// with a second token, adds one and is counted.
#[must_use]
pub fn added_mentions(before: &str, after: &str) -> Vec<PersonId> {
    use std::collections::HashMap;
    let now = mentions(after);
    if now.is_empty() {
        return Vec::new();
    }
    let mut had: HashMap<PersonId, usize> = HashMap::new();
    for m in mentions(before) {
        *had.entry(m.person_id).or_default() += 1;
    }
    let mut has: HashMap<PersonId, usize> = HashMap::new();
    let mut added = Vec::new();
    for m in now {
        let count = has.entry(m.person_id).or_default();
        *count += 1;
        if *count > had.get(&m.person_id).copied().unwrap_or(0) && !added.contains(&m.person_id) {
            added.push(m.person_id);
        }
    }
    added
}

/// `name` as a label: the characters the token's syntax needs are
/// replaced, line breaks and control characters become spaces, and runs
/// of white space collapse. A name with nothing left is "person".
#[must_use]
pub fn mention_label(name: &str) -> String {
    let swapped: String = name
        .chars()
        .map(|c| match c {
            '[' => '(',
            ']' => ')',
            '`' => '\'',
            '\\' | '|' => '/',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let label = swapped.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.is_empty() {
        "person".to_string()
    } else {
        label
    }
}

/// The token for a mention of the person `id`, labelled with `name`.
/// [`mentions`] reads back what this writes, for any name.
#[must_use]
pub fn mention_token(name: &str, id: PersonId) -> String {
    format!(
        "{OPEN}{}{MIDDLE}{})",
        mention_label(name),
        id.0.hyphenated()
    )
}

/// `text` with every mention reduced to `@label`: for a place that
/// shows text as plain words and has no names to resolve (a search
/// snippet, an excerpt).
#[must_use]
pub fn plain(text: &str) -> String {
    replace_mentions(text, |m| format!("@{}", m.label))
}

/// `text` with the labels of the author's own people brought up to
/// their current names, and how many labels changed. `None` when every
/// label of theirs is current.
///
/// Only people in `own_names` are touched: someone else's label is what
/// that person wrote, and their names are not this author's to know.
/// The result is offered to the author as an edit; nothing rewrites
/// text on its own.
#[must_use]
pub fn refresh_labels(text: &str, own_names: &[(PersonId, String)]) -> Option<(String, usize)> {
    let mut changed = 0;
    let refreshed = replace_mentions(text, |m| {
        let current = own_names
            .iter()
            .find(|(id, _)| *id == m.person_id)
            .map(|(_, name)| mention_label(name));
        match current {
            Some(label) if label != m.label => {
                changed += 1;
                mention_token(&label, m.person_id)
            }
            _ => text.get(m.range.clone()).unwrap_or_default().to_string(),
        }
    });
    (changed > 0).then_some((refreshed, changed))
}

/// `text` as HTML in which nothing is markup, for a name put into
/// Markdown before it is rendered: every character that is not a letter
/// or a digit is written as a numeric entity. A name such as
/// `[x](https://e)` or `*a*` would otherwise be read as Markdown inside
/// the mention.
#[must_use]
pub fn html_inert(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else {
            out.push_str(&format!("&#{};", u32::from(c)));
        }
    }
    out
}

// ── The picker ─────────────────────────────────────────────────────────

/// JavaScript counts string positions in UTF-16 code units and Rust in
/// UTF-8 bytes. Slicing by a UTF-16 offset can land inside a character
/// and panic (right after an emoji, a dash, a curly quote); in the
/// browser that panic stops every later keystroke. Every cursor value
/// from `selectionStart` goes through this before it indexes `text`.
#[must_use]
pub fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_idx, ch) in text.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    text.len()
}

/// The inverse: a UTF-8 byte offset as a UTF-16 offset, for
/// `setSelectionRange`. An offset inside a character counts up to the
/// start of that character.
#[must_use]
pub fn utf8_to_utf16_offset(text: &str, byte_offset: usize) -> usize {
    let mut cut = byte_offset.min(text.len());
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.get(..cut).unwrap_or_default().encode_utf16().count()
}

/// The longest thing typed after `@` that the picker still follows.
/// Past it, the `@` was not the start of a mention.
pub const MAX_QUERY_CHARS: usize = 40;

/// A mention being typed: where its `@` is, and what follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionQuery {
    /// Byte offset of the `@`.
    pub at: usize,
    /// What is typed between the `@` and the cursor. May be empty.
    pub query: String,
}

/// The mention being typed at the cursor, if any. `cursor` is a UTF-16
/// offset, as `selectionStart` gives it.
///
/// - The `@` is on the cursor's line, at the start of the text or after
///   white space or an opening bracket or quote. So an email address
///   typed in text (`ada@example.com`) opens nothing.
/// - What follows may hold spaces (names do), but does not start with
///   one, does not start a token (`@[`), and is at most
///   [`MAX_QUERY_CHARS`] long.
/// - Not inside code.
#[must_use]
pub fn mention_query_at(text: &str, cursor: usize) -> Option<MentionQuery> {
    let cut = utf16_to_utf8_offset(text, cursor);
    let before = text.get(..cut)?;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.get(line_start..)?;
    let at = line_start + line.rfind('@')?;
    let query = before.get(at + 1..)?;
    let opens = before.get(..at)?.chars().next_back().is_none_or(|c| {
        c.is_whitespace() || matches!(c, '(' | '[' | '{' | '"' | '\'' | '\u{201c}' | '\u{2018}')
    });
    if !opens
        || query.starts_with(char::is_whitespace)
        || query.starts_with('[')
        || query.chars().count() > MAX_QUERY_CHARS
        || code_ranges(text).iter().any(|r| r.contains(&at))
    {
        return None;
    }
    Some(MentionQuery {
        at,
        query: query.to_string(),
    })
}

/// `text` with the mention being typed (`typed`, from
/// [`mention_query_at`] at this `cursor`) replaced by the token for the
/// person picked, and the UTF-16 cursor position after it. A space
/// follows the token unless white space is there already.
#[must_use]
pub fn insert_mention(
    text: &str,
    typed: &MentionQuery,
    cursor: usize,
    name: &str,
    id: PersonId,
) -> (String, usize) {
    let cut = utf16_to_utf8_offset(text, cursor).max(typed.at);
    let head = text.get(..typed.at).unwrap_or_default();
    let tail = text.get(cut..).unwrap_or_default();
    let space = if tail.starts_with(char::is_whitespace) {
        ""
    } else {
        " "
    };
    let inserted = format!("{}{space}", mention_token(name, id));
    let after = head.len() + inserted.len();
    let out = format!("{head}{inserted}{tail}");
    let cursor = utf8_to_utf16_offset(&out, after);
    (out, cursor)
}

/// `byte_offset`, moved to the end of the mention it is inside of, if
/// it is strictly inside one. Text put in at a point inside a token (a
/// pasted image's placeholder) would un-make the mention.
#[must_use]
pub fn outside_a_token(text: &str, byte_offset: usize) -> usize {
    mentions(text)
        .into_iter()
        .find(|m| m.range.start < byte_offset && byte_offset < m.range.end)
        .map_or(byte_offset, |m| m.range.end)
}

// ── What the API answers with ──────────────────────────────────────────

/// A person a text mentions, as its reader may know them: the name, and
/// whether the person is the reader's own (then the mention is a link
/// to their page). Nothing else of the person, for anyone: a reader who
/// is not their owner gets the name and no more (ONTOLOGY §14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MentionName {
    pub person_id: PersonId,
    pub display_name: String,
    pub mine: bool,
}

/// "Mentioned in": one node that mentions a person, in its body, in its
/// notes, or both. Only nodes the reader can open are listed, and
/// `notes` counts only notes of such a node that are not in the trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PersonMention {
    pub node_id: NodeId,
    pub node_title: String,
    pub node_type: NodeType,
    pub in_body: bool,
    pub notes: u32,
}

/// One stored mention, as a backup holds it: a person, and the node
/// body or the note that mentions them (exactly one of the two).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BackupMention {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<NoteId>,
    pub person_id: PersonId,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "3f2a6b10-7c1d-4e2f-8a9b-0c1d2e3f4a5b";
    const BEA: &str = "11111111-2222-4333-8444-555555555555";

    fn id(s: &str) -> PersonId {
        PersonId(Uuid::parse_str(s).expect("uuid"))
    }

    fn found(text: &str) -> Vec<(String, PersonId)> {
        mentions(text)
            .into_iter()
            .map(|m| (m.label.to_string(), m.person_id))
            .collect()
    }

    #[test]
    fn a_mention_is_a_label_and_an_id() {
        let text = format!("Met @[Ada Lovelace](person:{ADA}) and @[ Bea ](person:{BEA}).");
        assert_eq!(
            found(&text),
            [
                ("Ada Lovelace".to_string(), id(ADA)),
                ("Bea".to_string(), id(BEA))
            ]
        );
        // The range covers the whole token, `@` to `)`.
        let first = &mentions(&text)[0];
        assert_eq!(
            &text[first.range.clone()],
            format!("@[Ada Lovelace](person:{ADA})")
        );
        assert_eq!(mentioned_ids(&format!("{text} {text}")), [id(ADA), id(BEA)]);
    }

    #[test]
    fn a_save_adds_a_mention_only_when_it_has_one_more() {
        let ada = format!("@[Ada](person:{ADA})");
        let bea = format!("@[Bea](person:{BEA})");
        // New in the text.
        assert_eq!(added_mentions("", &ada), [id(ADA)]);
        assert_eq!(
            added_mentions("plain", &format!("{ada} {bea}")),
            [id(ADA), id(BEA)]
        );
        // There already (planted, perhaps): an edit around it adds nothing,
        // nor does moving it, relabelling it or taking one of two out.
        let planted = format!("<!-- {ada} -->");
        assert_eq!(
            added_mentions(&planted, &format!("{planted} and a word")),
            []
        );
        assert_eq!(
            added_mentions(&format!("a {ada} b"), &format!("b {ada} a")),
            []
        );
        assert_eq!(
            added_mentions(&ada, &format!("@[Ada King](person:{ADA})")),
            []
        );
        assert_eq!(added_mentions(&format!("{ada} {ada}"), &ada), []);
        // One more of the same person is the saver mentioning them.
        assert_eq!(
            added_mentions(&planted, &format!("{planted} {ada}")),
            [id(ADA)]
        );
        // Only the person who gained one.
        assert_eq!(
            added_mentions(&format!("{ada} {bea}"), &format!("{ada} {bea} {bea}")),
            [id(BEA)]
        );
        // Nothing mentioned now: nothing added, whatever was there.
        assert_eq!(added_mentions(&ada, "gone"), []);
    }

    #[test]
    fn one_spelling_of_an_id() {
        let upper = ADA.to_uppercase();
        for not_one in [
            format!("@[Ada](person:{upper})"),
            format!("@[Ada](person:{{{ADA}}})"),
            format!("@[Ada](person:{})", ADA.replace('-', "")),
            format!("@[Ada](person:{ADA}"),
            format!("@[Ada](person:{ADA}x)"),
            format!("@[Ada](people:{ADA})"),
            format!("@[Ada] (person:{ADA})"),
            format!("@ [Ada](person:{ADA})"),
            format!("[Ada](person:{ADA})"),
            "@[Ada](person:not-an-id-at-all-but-36-characters!!)".to_string(),
        ] {
            assert!(mentions(&not_one).is_empty(), "{not_one}");
        }
    }

    #[test]
    fn a_label_is_one_line_with_no_brackets_and_is_not_empty() {
        for not_one in [
            format!("@[](person:{ADA})"),
            format!("@[  ](person:{ADA})"),
            format!("@[A[da](person:{ADA})"),
            format!("@[A`da](person:{ADA})"),
            format!("@[Ada\nLovelace](person:{ADA})"),
        ] {
            assert!(mentions(&not_one).is_empty(), "{not_one}");
        }
        // A bracket closes the label: what follows is not a mention.
        assert!(mentions(&format!("@[A]da](person:{ADA})")).is_empty());
        // Scanning goes on after something that was not one.
        let text = format!("@[x] then @[Ada](person:{ADA})");
        assert_eq!(found(&text), [("Ada".to_string(), id(ADA))]);
        // A round bracket in a name is no trouble.
        let text = format!("@[Ada (the first)](person:{ADA})");
        assert_eq!(found(&text), [("Ada (the first)".to_string(), id(ADA))]);
    }

    #[test]
    fn nothing_inside_code_or_a_wiki_link_is_a_mention() {
        let token = format!("@[Ada](person:{ADA})");
        assert!(mentions(&format!("`{token}`")).is_empty());
        assert!(mentions(&format!("```\n{token}\n```\n")).is_empty());
        assert!(mentions(&format!("[[Notes|{token}]]")).is_empty());
        // Outside them it is one.
        let text = format!("`code` {token} [[Notes]]");
        assert_eq!(mentions(&text).len(), 1);
        // A wiki link right after an `@` is a wiki link.
        assert!(mentions("@[[Ada]]").is_empty());
    }

    #[test]
    fn a_token_reads_back_for_any_name() {
        for name in [
            "Ada Lovelace",
            "  Ada   Lovelace ",
            "A[d]a",
            "Ada `the` first",
            "Ada | Bea \\ Cy",
            "Ada\nLovelace\tKing",
            "[x](https://example.com)",
            "*bold* _name_ <b>",
            "José Ångström 李",
            "",
            " \n ",
            "]](person:x)",
        ] {
            let token = mention_token(name, id(ADA));
            let read = mentions(&token);
            assert_eq!(read.len(), 1, "{name:?} -> {token}");
            assert_eq!(read[0].person_id, id(ADA));
            assert_eq!(read[0].range, 0..token.len(), "{token}");
            assert_eq!(read[0].label, mention_label(name));
        }
        assert_eq!(mention_label("  Ada   Lovelace "), "Ada Lovelace");
        assert_eq!(mention_label("A[d]a"), "A(d)a");
        assert_eq!(mention_label(""), "person");
    }

    #[test]
    fn replacing_keeps_everything_else() {
        let text = format!("a `@[x](person:{ADA})` b @[Ada](person:{ADA}) c — π");
        assert_eq!(
            replace_mentions(&text, |m| format!("<{}>", m.label)),
            format!("a `@[x](person:{ADA})` b <Ada> c — π")
        );
        assert_eq!(plain(&text), format!("a `@[x](person:{ADA})` b @Ada c — π"));
        assert_eq!(plain("no mention here"), "no mention here");
    }

    #[test]
    fn only_own_labels_are_refreshed() {
        let text = format!("@[Ada Lovelace](person:{ADA}) met @[Bea](person:{BEA}).");
        let own = [(id(ADA), "Ada King".to_string())];
        let (fresh, changed) = refresh_labels(&text, &own).expect("stale");
        assert_eq!(
            fresh,
            format!("@[Ada King](person:{ADA}) met @[Bea](person:{BEA}).")
        );
        assert_eq!(changed, 1);
        // Current already, or nobody of theirs: nothing to offer.
        assert_eq!(refresh_labels(&fresh, &own), None);
        assert_eq!(refresh_labels(&text, &[]), None);
        // A name that needs tidying counts as current once tidied.
        let odd = [(id(ADA), " Ada  [King] ".to_string())];
        let (fresh, _) = refresh_labels(&text, &odd).expect("stale");
        assert!(fresh.starts_with("@[Ada (King)](person:"), "{fresh}");
        assert_eq!(refresh_labels(&fresh, &odd), None);
    }

    #[test]
    fn a_name_is_never_markup() {
        assert_eq!(html_inert("Ada"), "Ada");
        assert_eq!(html_inert("Ada L."), "Ada&#32;L&#46;");
        let hostile = html_inert("[x](https://e) *a* <b> &amp; `c`");
        for c in ['[', ']', '(', ')', '*', '<', '>', '`', ' ', '_'] {
            assert!(!hostile.contains(c), "{c:?} in {hostile}");
        }
        // Letters of any script stay readable.
        assert_eq!(html_inert("José李"), "José李");
    }

    #[test]
    fn the_picker_follows_an_at_sign_that_starts_a_word() {
        let q = |text: &str| mention_query_at(text, text.encode_utf16().count());
        assert_eq!(
            q("@"),
            Some(MentionQuery {
                at: 0,
                query: String::new()
            })
        );
        assert_eq!(
            q("Met @ad").map(|m| (m.at, m.query)),
            Some((4, "ad".into()))
        );
        assert_eq!(
            q("Met @Ada Love").map(|m| m.query),
            Some("Ada Love".to_string())
        );
        assert_eq!(q("(@a").map(|m| m.query), Some("a".to_string()));
        assert_eq!(q("line one\n@b").map(|m| m.at), Some(9));
        // An email address in text opens nothing.
        assert_eq!(q("write to ada@exam"), None);
        // Nor a bare "@ ", a finished token, or the line before.
        assert_eq!(q("Met @ "), None);
        assert_eq!(q(&format!("@[Ada](person:{ADA})")), None);
        assert_eq!(q("@[Ad"), None);
        assert_eq!(q("@ada\nnext line"), None);
        // Nor in code (the cursor is inside the span).
        assert_eq!(mention_query_at("`@a`", 3), None);
        assert_eq!(q("```\n@a"), None);
        // Past the limit it was not a mention being typed.
        let long = format!("@{}", "a".repeat(MAX_QUERY_CHARS));
        assert!(q(&long).is_some());
        assert_eq!(q(&format!("{long}a")), None);
    }

    #[test]
    fn the_cursor_is_counted_in_utf16_and_never_splits_a_character() {
        // "👉" is two UTF-16 units and four bytes.
        let text = "👉 @ad — π";
        let cursor = "👉 @ad".encode_utf16().count();
        let typed = mention_query_at(text, cursor).expect("typing");
        assert_eq!(typed.at, "👉 ".len());
        assert_eq!(typed.query, "ad");
        let (out, after) = insert_mention(text, &typed, cursor, "Ada", id(ADA));
        assert_eq!(out, format!("👉 @[Ada](person:{ADA}) — π"));
        assert_eq!(
            after,
            format!("👉 @[Ada](person:{ADA})").encode_utf16().count()
        );
        // A cursor past the end, and one inside a character, are safe.
        assert_eq!(utf16_to_utf8_offset("π", 99), 2);
        assert_eq!(utf8_to_utf16_offset("π", 1), 0);
        assert_eq!(utf8_to_utf16_offset("👉a", 4), 2);
        assert!(mention_query_at("👉", 1).is_none());
    }

    #[test]
    fn picking_puts_the_token_where_the_query_was() {
        let text = "Met @ad";
        let cursor = text.len();
        let typed = mention_query_at(text, cursor).expect("typing");
        let (out, after) = insert_mention(text, &typed, cursor, "Ada Lovelace", id(ADA));
        assert_eq!(out, format!("Met @[Ada Lovelace](person:{ADA}) "));
        assert_eq!(after, out.len());
        // Mid-text: the rest is kept, and no second space is added.
        let text = "Met @ad yesterday";
        let cursor = "Met @ad".len();
        let typed = mention_query_at(text, cursor).expect("typing");
        let (out, after) = insert_mention(text, &typed, cursor, "Ada", id(ADA));
        assert_eq!(out, format!("Met @[Ada](person:{ADA}) yesterday"));
        assert_eq!(after, format!("Met @[Ada](person:{ADA})").len());
        assert_eq!(mentions(&out).len(), 1);
    }

    #[test]
    fn a_point_inside_a_token_moves_to_its_end() {
        let text = format!("a @[Ada](person:{ADA}) b");
        let token_end = text.len() - 2;
        assert_eq!(outside_a_token(&text, 5), token_end);
        assert_eq!(outside_a_token(&text, 2), 2, "at its start");
        assert_eq!(outside_a_token(&text, token_end), token_end);
        assert_eq!(outside_a_token(&text, 1), 1);
    }

    #[test]
    fn what_another_reader_gets_is_a_name_and_no_more() {
        let name = MentionName {
            person_id: id(ADA),
            display_name: "Ada".into(),
            mine: false,
        };
        let json = serde_json::to_value(&name).expect("json");
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["display_name", "mine", "person_id"]);
    }
}
