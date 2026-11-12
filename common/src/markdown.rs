//! Lightweight markdown utilities for extracting structured content.
//!
//! These helpers operate on raw markdown text without requiring a full parser,
//! keeping the `common` crate dependency-free of `pulldown-cmark`.

/// A plain-text preview of a markdown body: its text lines (no headings,
/// code fences or rules) joined with spaces, cut to `max_chars` characters
/// with "…" when longer. `None` when nothing is left. A heuristic, not a
/// parser (this module has none); the graph's hover card shows it as plain
/// text.
///
/// Each line is reduced by `note::plain_line`, the stripper a note's
/// collapsed title uses (#281, 3.4): links and wiki links keep what they
/// show, emphasis, strikethrough and code marks go, and so do list markers
/// and quote marks. This function had its own, which left all of those
/// but `**`, `__` and backticks in the text.
pub fn plain_preview(body: &str, max_chars: usize) -> Option<String> {
    let clean = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !crate::note::is_fence_or_rule(l))
        .map(crate::note::plain_line)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if clean.is_empty() {
        return None;
    }
    let mut chars = clean.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    Some(if chars.next().is_some() {
        format!("{taken}\u{2026}")
    } else {
        taken
    })
}

/// Returns the ATX heading level (1–6) if `line` starts with 1–6 `#` chars
/// followed by a space or end-of-line.  Returns `None` otherwise.
pub(crate) fn atx_heading_level(line: &str) -> Option<usize> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    // Must be followed by a space or be end-of-line (bare `###`).
    let rest = &line[hashes..];
    if rest.is_empty() || rest.starts_with(' ') {
        Some(hashes)
    } else {
        None
    }
}

/// Convert the first plain-text occurrence of `title` in `body` into a
/// wiki-link ("unlinked mention" → link).
///
/// - ASCII-case-insensitive, but the match must sit on word boundaries so
///   `alpha` never rewrites `alphabet`.
/// - Occurrences inside an existing `[[...]]` span (target or alias text)
///   are skipped.
/// - When the prose casing differs from `title`, it is preserved through the
///   alias form: `[[Title|matched text]]`.
///
/// Returns `None` when nothing linkable is found — e.g. the caller's search
/// hit was a stemmed or fuzzy match rather than a literal one.
pub fn link_first_mention(body: &str, title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }

    // Off-limits for rewriting: existing links, by the one scanner (#281,
    // 3.1), and code, where a `[[…]]` would be shown as typed.
    let link_spans: Vec<(usize, usize)> = crate::wikilink::wikilinks(body)
        .into_iter()
        .map(|link| link.range)
        .chain(crate::wikilink::code_ranges(body))
        .map(|r| (r.start, r.end))
        .collect();

    // ASCII case folding is byte-for-byte, so offsets into the folded copies
    // are valid for the originals (and char boundaries are preserved).
    let body_fold = body.to_ascii_lowercase();
    let title_fold = title.to_ascii_lowercase();

    let mut from = 0;
    while let Some(rel) = body_fold[from..].find(&title_fold) {
        let start = from + rel;
        let end = start + title.len();
        let inside_link = link_spans.iter().any(|&(s, e)| start >= s && start < e);
        let boundary_before = !body[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric());
        let boundary_after = !body[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric());
        if !inside_link && boundary_before && boundary_after {
            let matched = &body[start..end];
            let replacement = if matched == title {
                format!("[[{title}]]")
            } else {
                format!("[[{title}|{matched}]]")
            };
            let mut out = String::with_capacity(body.len() + replacement.len());
            out.push_str(&body[..start]);
            out.push_str(&replacement);
            out.push_str(&body[end..]);
            return Some(out);
        }
        from = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_mention_exact_case() {
        assert_eq!(
            link_first_mention("Alpha is key", "Alpha"),
            Some("[[Alpha]] is key".to_string())
        );
    }

    #[test]
    fn link_mention_preserves_prose_casing_via_alias() {
        assert_eq!(
            link_first_mention("see alpha note", "Alpha"),
            Some("see [[Alpha|alpha]] note".to_string())
        );
    }

    #[test]
    fn link_mention_skips_existing_wikilinks() {
        assert_eq!(
            link_first_mention("[[Alpha]] and alpha again", "Alpha"),
            Some("[[Alpha]] and [[Alpha|alpha]] again".to_string())
        );
    }

    #[test]
    fn link_mention_leaves_code_alone() {
        // A mention inside code is shown as typed; linking it there would
        // show the brackets (#281, 3.1 and 1.10).
        assert_eq!(link_first_mention("run `alpha --fast`", "Alpha"), None);
        assert_eq!(
            link_first_mention("`alpha` is the alpha tool", "Alpha"),
            Some("`alpha` is the [[Alpha|alpha]] tool".to_string())
        );
        assert_eq!(link_first_mention("```\nalpha\n```", "Alpha"), None);
    }

    #[test]
    fn link_mention_requires_word_boundaries() {
        assert_eq!(link_first_mention("the alphabet song", "alpha"), None);
        assert_eq!(link_first_mention("realpha", "alpha"), None);
    }

    #[test]
    fn link_mention_none_when_absent_or_empty() {
        assert_eq!(link_first_mention("nothing here", "Alpha"), None);
        assert_eq!(link_first_mention("anything", ""), None);
        assert_eq!(link_first_mention("anything", "   "), None);
    }

    #[test]
    fn link_mention_survives_multibyte_text() {
        assert_eq!(
            link_first_mention("café — alpha — π", "Alpha"),
            Some("café — [[Alpha|alpha]] — π".to_string())
        );
    }

    #[test]
    fn link_mention_skips_alias_display_text() {
        // "alpha" inside the display part of an existing link is off-limits.
        assert_eq!(
            link_first_mention("[[Other|alpha thing]] end", "Alpha"),
            None
        );
    }

    #[test]
    fn a_plain_preview_reads_like_the_rendered_text() {
        // #281, 3.4: the graph's hover card showed `[[T|x]]`, link syntax,
        // `~~`, list markers and `>` as typed.
        let body = "> Ask [[Alice Smith|Alice]] about the [plan](https://example.com).\n\
                    - a ~~dropped~~ item\n1. first *step*";
        assert_eq!(
            plain_preview(body, 100).as_deref(),
            Some("Ask Alice about the plan. a dropped item first step")
        );
    }

    #[test]
    fn a_plain_preview_keeps_text_lines_only() {
        let body =
            "# Heading\n\nSome **bold** text.\n\n```\ncode\n```\n---\nMore `code` and __more__.";
        // The fence's inner line is text to this heuristic, as it was in the
        // graph view it came from.
        assert_eq!(
            plain_preview(body, 100).as_deref(),
            Some("Some bold text. code More code and more.")
        );
    }

    #[test]
    fn a_plain_preview_of_nothing_is_none() {
        assert_eq!(plain_preview("", 100), None);
        assert_eq!(plain_preview("# Only a heading\n\n---\n", 100), None);
    }

    #[test]
    fn a_plain_preview_cuts_by_characters_not_bytes() {
        let body = "é".repeat(150);
        let p = plain_preview(&body, 100).unwrap_or_default();
        assert_eq!(p.chars().count(), 101, "100 characters and the ellipsis");
        assert!(p.ends_with('\u{2026}'));
        assert_eq!(plain_preview("short", 100).as_deref(), Some("short"));
        assert_eq!(
            plain_preview(&"x".repeat(100), 100).map(|p| p.len()),
            Some(100)
        );
    }
}
