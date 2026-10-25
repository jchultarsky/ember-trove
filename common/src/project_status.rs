//! A project's status as one structured value (#143).
//!
//! It used to live as prose in a `## Status` section of the body. Now it's
//! a short token on the node (`nodes.project_status`): open-ended values
//! (the picker suggests ones already used), with longer narrative left in
//! the body or notes. It's named `project_status` because `nodes.status`
//! is the Draft / Published / Archived lifecycle.
//!
//! [`normalize`] is the one validation rule (API, picker and database
//! `CHECK` agree on it). [`split_from_body`] moves a legacy status token
//! out of a body; the one-time backfill and project creation both use it.

use crate::markdown::atx_heading_level;

/// The longest status, in characters. Statuses are tokens ("Active", "On
/// hold"); the list UI and filters depend on them staying short.
pub const MAX_LEN: usize = 40;

/// A new project's status when nothing else gives one: what the project
/// scaffold's `## Status` section used to say.
pub const DEFAULT: &str = "Active";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusError {
    /// Longer than [`MAX_LEN`] characters.
    TooLong,
    /// Contains a line break or another control character.
    ControlChar,
}

impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatusError::TooLong => write!(f, "status must be at most {MAX_LEN} characters"),
            StatusError::ControlChar => write!(f, "status must be a single line of text"),
        }
    }
}

/// Validate and tidy a typed status: trim, collapse inner whitespace to one
/// space. Blank means "no status" (`Ok(None)`).
pub fn normalize(raw: &str) -> Result<Option<String>, StatusError> {
    if raw.chars().any(|c| c.is_control() && c != '\t') {
        return Err(StatusError::ControlChar);
    }
    let tidy = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if tidy.is_empty() {
        return Ok(None);
    }
    if tidy.chars().count() > MAX_LEN {
        return Err(StatusError::TooLong);
    }
    Ok(Some(tidy))
}

/// The case-insensitive identity of a status: "Active" and "ACTIVE" are one
/// value for suggestions, matching and the list filter.
pub fn key(status: &str) -> String {
    status.to_lowercase()
}

/// A status token moved out of a body: the token, and the body without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub status: String,
    pub body: String,
}

/// Move a legacy status token out of `body`. Deliberately conservative,
/// because it rewrites user content: returns `None`, and the body stays
/// as it is, unless all of these hold.
/// - A heading's text is exactly "Status" or "Project Status" (any level,
///   any case; "Status Update" doesn't count).
/// - The first non-blank line under it is a token: `**Active**`,
///   `- Active` or `Active`, alone in its paragraph, at most [`MAX_LEN`]
///   characters, with no links or other markup.
///
/// Only the token line goes, and narrative under the heading stays. The
/// heading goes too when nothing is left under it.
pub fn split_from_body(body: &str) -> Option<Split> {
    // Lines with their endings, so the rebuilt body keeps CRLF intact.
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let blank = |l: &str| l.trim().is_empty();

    let (heading, level) = lines.iter().enumerate().find_map(|(i, l)| {
        let t = l.trim();
        let level = atx_heading_level(t)?;
        let text = t[level..]
            .trim()
            .trim_end_matches('#')
            .trim()
            .to_lowercase();
        (text == "status" || text == "project status").then_some((i, level))
    })?;

    // The token line: the first non-blank line under the heading, alone in
    // its paragraph (next line blank, a heading, or the end).
    let token_at = (heading + 1..lines.len()).find(|&i| !blank(lines[i]))?;
    let line = lines[token_at].trim();
    if atx_heading_level(line).is_some() {
        return None;
    }
    if let Some(next) = lines.get(token_at + 1)
        && !blank(next)
        && atx_heading_level(next.trim()).is_none()
    {
        return None;
    }
    let status = token(line)?;

    // Drop the token line and the blank lines after it.
    let mut after = token_at + 1;
    while after < lines.len() && blank(lines[after]) {
        after += 1;
    }
    // The section ends at the next heading of the same or a higher level.
    let section_end = (after..lines.len())
        .find(|&i| atx_heading_level(lines[i].trim()).is_some_and(|l| l <= level))
        .unwrap_or(lines.len());
    let narrative_left = lines[after..section_end].iter().any(|l| !blank(l));

    let mut out = String::with_capacity(body.len());
    if narrative_left {
        lines[..token_at].iter().for_each(|l| out.push_str(l));
    } else {
        // Nothing left under the heading: it goes too.
        lines[..heading].iter().for_each(|l| out.push_str(l));
    }
    lines[after..].iter().for_each(|l| out.push_str(l));
    Some(Split { status, body: out })
}

/// The status in a candidate token line (`**Active**`, `- Active`, `Active`),
/// or `None` if the line is prose or markup rather than a token.
fn token(line: &str) -> Option<String> {
    let mut t = line;
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(bullet) {
            t = rest.trim();
            break;
        }
    }
    for mark in ["**", "__"] {
        if let Some(inner) = t.strip_prefix(mark).and_then(|r| r.strip_suffix(mark)) {
            t = inner.trim();
            break;
        }
    }
    // Leftover markup means this isn't a plain token (links, code, emphasis…).
    if t.chars()
        .any(|c| matches!(c, '[' | ']' | '<' | '>' | '`' | '|' | '*' | '#'))
    {
        return None;
    }
    normalize(t).ok().flatten()
}

/// One row of the status picker's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOption {
    /// A status already in use, spelled as stored.
    Existing(String),
    /// The typed text as a new status (already normalized).
    New(String),
}

impl PickerOption {
    pub fn value(&self) -> &str {
        match self {
            PickerOption::Existing(s) | PickerOption::New(s) => s,
        }
    }
}

/// The status picker's options for `query` (#143). Suggestions that contain
/// it, case-insensitively, come first, exact then prefix then the rest,
/// each in the given (most-used-first) order. Then the typed value as a new
/// status, but only when it's valid and isn't an existing one in another
/// case: "active" offers the existing "Active", never a second spelling.
pub fn picker_options(query: &str, suggestions: &[String]) -> Vec<PickerOption> {
    let typed = normalize(query).ok().flatten();
    let q = typed.as_deref().map(key).unwrap_or_default();
    // Rank 0 exact, 1 prefix, 2 substring; a stable sort keeps the order.
    let mut ranked: Vec<(u8, &String)> = suggestions
        .iter()
        .filter_map(|s| {
            let k = key(s);
            let rank = if k == q {
                0
            } else if k.starts_with(&q) {
                1
            } else if k.contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank, s))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    let exists = ranked.first().is_some_and(|(rank, _)| *rank == 0);
    let mut out: Vec<PickerOption> = ranked
        .into_iter()
        .map(|(_, s)| PickerOption::Existing(s.clone()))
        .collect();
    if let Some(t) = typed
        && !exists
    {
        out.push(PickerOption::New(t));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sugg(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn values(opts: &[PickerOption]) -> Vec<(&str, bool)> {
        opts.iter()
            .map(|o| (o.value(), matches!(o, PickerOption::New(_))))
            .collect()
    }

    #[test]
    fn an_empty_query_lists_every_suggestion_in_order() {
        let s = sugg(&["Active", "On hold", "Done"]);
        assert_eq!(
            values(&picker_options("", &s)),
            [("Active", false), ("On hold", false), ("Done", false)]
        );
    }

    #[test]
    fn matches_rank_exact_then_prefix_then_substring() {
        let s = sugg(&["Waiting on legal", "On hold", "Holding pattern", "Hold"]);
        // "hold" is the existing "Hold" in another case: no new option.
        assert_eq!(
            values(&picker_options("hold", &s)),
            [
                ("Hold", false),
                ("Holding pattern", false),
                ("On hold", false)
            ]
        );
    }

    #[test]
    fn a_new_value_is_offered_after_the_matches() {
        let s = sugg(&["Active", "Inactive"]);
        assert_eq!(
            values(&picker_options("  act  ", &s)),
            [("Active", false), ("Inactive", false), ("act", true)]
        );
    }

    #[test]
    fn a_case_variant_of_an_existing_status_offers_only_the_existing_one() {
        let s = sugg(&["Active"]);
        assert_eq!(values(&picker_options("ACTIVE", &s)), [("Active", false)]);
    }

    #[test]
    fn an_invalid_typed_value_is_not_offered() {
        assert!(picker_options(&"x".repeat(41), &[]).is_empty());
    }

    fn split(body: &str) -> Option<(String, String)> {
        split_from_body(body).map(|s| (s.status, s.body))
    }

    #[test]
    fn normalize_trims_collapses_and_blank_clears() {
        assert_eq!(normalize("  On   hold \t"), Ok(Some("On hold".into())));
        assert_eq!(normalize(""), Ok(None));
        assert_eq!(normalize("   "), Ok(None));
    }

    #[test]
    fn normalize_limits_length_in_characters_not_bytes() {
        let forty = "é".repeat(MAX_LEN);
        assert_eq!(normalize(&forty), Ok(Some(forty.clone())));
        assert_eq!(normalize(&format!("{forty}x")), Err(StatusError::TooLong));
    }

    #[test]
    fn normalize_rejects_line_breaks_and_control_characters() {
        assert_eq!(normalize("Active\nmore"), Err(StatusError::ControlChar));
        assert_eq!(normalize("Act\u{0007}ive"), Err(StatusError::ControlChar));
    }

    #[test]
    fn key_is_case_insensitive() {
        assert_eq!(key("ACTIVE"), key("Active"));
    }

    #[test]
    fn the_default_scaffold_bullet_moves_out_with_its_heading() {
        let body = "## Status\n\n- Active\n\n## Goals\n\n-\n";
        assert_eq!(
            split(body),
            Some(("Active".into(), "## Goals\n\n-\n".into()))
        );
    }

    #[test]
    fn a_bold_token_moves_out_and_the_narrative_stays() {
        let body = "Intro.\n\n## Status\n\n**ACTIVE**\n\nWaiting on legal.\n\n## Team\n";
        assert_eq!(
            split(body),
            Some((
                "ACTIVE".into(),
                "Intro.\n\n## Status\n\nWaiting on legal.\n\n## Team\n".into()
            ))
        );
    }

    #[test]
    fn project_status_heading_and_a_plain_token_any_level() {
        assert_eq!(
            split("### project status\n\nOn hold\n"),
            Some(("On hold".into(), String::new()))
        );
    }

    #[test]
    fn crlf_bodies_keep_their_line_endings() {
        let body = "## Status\r\n\r\n- Active\r\n\r\nNotes here.\r\n";
        assert_eq!(
            split(body),
            Some(("Active".into(), "## Status\r\n\r\nNotes here.\r\n".into()))
        );
    }

    #[test]
    fn lookalike_headings_are_not_status() {
        assert_eq!(split("## Status Update\n\nActive\n"), None);
        assert_eq!(split("## Statuses\n\nActive\n"), None);
        assert_eq!(split("No headings at all\n"), None);
    }

    #[test]
    fn prose_and_lists_are_not_tokens() {
        // A list of points is narrative (this is extract_section's example).
        assert_eq!(
            split("## Status\n\n- MVP shipped\n- Collecting feedback\n"),
            None
        );
        // A paragraph's first line isn't a token either.
        assert_eq!(split("## Status\n\nActive and going\nwell so far.\n"), None);
        // Too long to be a token.
        assert_eq!(
            split("## Status\n\nWe are currently waiting on the vendor's response\n"),
            None
        );
    }

    #[test]
    fn markup_and_links_are_not_tokens() {
        assert_eq!(split("## Status\n\n[[Other node]]\n"), None);
        assert_eq!(split("## Status\n\n[Active](https://x.test)\n"), None);
        assert_eq!(split("## Status\n\n`active`\n"), None);
    }

    #[test]
    fn an_empty_or_nested_section_is_left_alone() {
        assert_eq!(split("## Status\n\n## Goals\n"), None);
        assert_eq!(split("## Status\n\n### Detail\n\nActive\n"), None);
        assert_eq!(split("## Status\n"), None);
    }

    #[test]
    fn only_the_first_status_section_is_touched() {
        let body = "## Status\n\nActive\n\n## Status\n\nDone\n";
        assert_eq!(
            split(body),
            Some(("Active".into(), "## Status\n\nDone\n".into()))
        );
    }
}
