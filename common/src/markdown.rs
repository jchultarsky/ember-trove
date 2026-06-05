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
}
