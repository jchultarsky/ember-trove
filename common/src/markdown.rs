//! Lightweight markdown utilities for extracting structured content.
//!
//! These helpers operate on raw markdown text without requiring a full parser,
//! keeping the `common` crate dependency-free of `pulldown-cmark`.

/// Extract the content under a markdown heading whose text matches
/// `heading` (case-insensitive).  Returns everything from the line after
/// the heading up to (but not including) the next heading of equal or
/// higher level, trimmed of leading/trailing blank lines.
///
/// Supports ATX headings (`#`–`######`).  The match is flexible:
/// `"Status"` matches `## Status`, `## Project Status`, `### Current Status`, etc.
///
/// Returns `None` if no matching heading is found or the section body is empty.
pub fn extract_section(body: &str, heading: &str) -> Option<String> {
    let heading_lower = heading.to_lowercase();
    let mut found_level: Option<usize> = None;
    let mut lines: Vec<&str> = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(level) = atx_heading_level(trimmed) {
            if let Some(fl) = found_level {
                // We're already collecting — stop at equal or higher level heading.
                if level <= fl {
                    break;
                }
                // Lower-level sub-heading inside the section — include it.
                lines.push(line);
            } else {
                // Not yet collecting — check if this heading matches.
                let text = trimmed[level..]
                    .trim_start_matches(' ')
                    .trim_end_matches('#')
                    .trim();
                if text.to_lowercase().contains(&heading_lower) {
                    found_level = Some(level);
                }
            }
        } else if found_level.is_some() {
            lines.push(line);
        }
    }

    // Trim leading/trailing blank lines.
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        return None;
    }

    Some(lines.join("\n"))
}

/// Returns the ATX heading level (1–6) if `line` starts with 1–6 `#` chars
/// followed by a space or end-of-line.  Returns `None` otherwise.
fn atx_heading_level(line: &str) -> Option<usize> {
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

    // Byte spans of existing [[...]] links — off-limits for rewriting.
    let mut link_spans: Vec<(usize, usize)> = Vec::new();
    let mut scan = 0;
    while let Some(open_rel) = body[scan..].find("[[") {
        let open = scan + open_rel;
        match body[open + 2..].find("]]") {
            Some(close_rel) => {
                let end = open + 2 + close_rel + 2;
                link_spans.push((open, end));
                scan = end;
            }
            None => break,
        }
    }

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
    fn extracts_status_section() {
        let body = "\
# My Project

Some intro text.

## Status

- MVP shipped
- Collecting feedback

## Tasks

- [ ] Do something
";
        let section = extract_section(body, "Status").expect("should find section");
        assert!(section.contains("MVP shipped"));
        assert!(section.contains("Collecting feedback"));
        assert!(
            !section.contains("Do something"),
            "should not leak into next section"
        );
    }

    #[test]
    fn case_insensitive_match() {
        let body = "## PROJECT STATUS\n\nAll good.\n";
        let section = extract_section(body, "status").expect("should match case-insensitively");
        assert_eq!(section, "All good.");
    }

    #[test]
    fn returns_none_when_no_match() {
        let body = "## Overview\n\nJust an overview.\n";
        assert!(extract_section(body, "Status").is_none());
    }

    #[test]
    fn returns_none_when_section_empty() {
        let body = "## Status\n\n## Next Section\n";
        assert!(extract_section(body, "Status").is_none());
    }

    #[test]
    fn stops_at_equal_level_heading() {
        let body = "\
## Status

In progress.

## Goals

Ship it.
";
        let section = extract_section(body, "Status").expect("should find");
        assert_eq!(section, "In progress.");
    }

    #[test]
    fn includes_sub_headings() {
        let body = "\
## Status

### Backend
Done.

### Frontend
WIP.

## Other
";
        let section = extract_section(body, "Status").expect("should find");
        assert!(section.contains("### Backend"));
        assert!(section.contains("### Frontend"));
        assert!(section.contains("Done."));
        assert!(section.contains("WIP."));
        assert!(!section.contains("Other"));
    }

    #[test]
    fn trims_surrounding_blanks() {
        let body = "## Status\n\n\n  content  \n\n\n## End\n";
        let section = extract_section(body, "Status").expect("should find");
        assert_eq!(section, "  content  ");
    }

    #[test]
    fn handles_heading_at_end_of_file() {
        let body = "## Status\n\nFinal line.";
        let section = extract_section(body, "Status").expect("should find");
        assert_eq!(section, "Final line.");
    }

    #[test]
    fn does_not_match_non_heading_hashes() {
        let body = "##Status\n\nBroken heading.\n## Status\n\nReal content.\n";
        let section = extract_section(body, "Status").expect("should find");
        assert_eq!(section, "Real content.");
    }

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
}
