//! Wiki-link (`[[title]]`) parsing, shared by the API (edge sync, title
//! resolution) and the UI (rendering) so both read links the same way
//! (#134; it used to live in the API only).

/// Extract all wiki-link target titles from a markdown body.
///
/// Handles both `[[title]]` and `[[title|display text]]` — only the target
/// title (before the `|`) is returned. Titles are trimmed of whitespace.
/// Duplicate titles are preserved; deduplication is the caller's concern.
pub fn parse_wikilink_titles(content: &str) -> Vec<String> {
    let mut titles = Vec::new();
    let mut remaining = content;

    while let Some(open) = remaining.find("[[") {
        remaining = &remaining[open + 2..];
        if let Some(close) = remaining.find("]]") {
            let inner = &remaining[..close];
            // Only look at the part before a pipe: [[target|display]]
            let target = inner.split('|').next().unwrap_or("").trim();
            if !target.is_empty() {
                titles.push(target.to_string());
            }
            remaining = &remaining[close + 2..];
        } else {
            // Unclosed [[  — stop scanning
            break;
        }
    }

    titles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_wikilink() {
        let titles = parse_wikilink_titles("See [[Rust Language]] for details.");
        assert_eq!(titles, vec!["Rust Language"]);
    }

    #[test]
    fn test_piped_wikilink() {
        let titles = parse_wikilink_titles("Check [[Rust Language|Rust]] out.");
        assert_eq!(titles, vec!["Rust Language"]);
    }

    #[test]
    fn test_multiple_wikilinks() {
        let titles = parse_wikilink_titles("[[Alpha]] and [[Beta|B]] and [[Gamma]].");
        assert_eq!(titles, vec!["Alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn test_unclosed_wikilink() {
        // Unclosed [[ should not panic and should stop scanning.
        let titles = parse_wikilink_titles("[[Alpha]] then [[unclosed");
        assert_eq!(titles, vec!["Alpha"]);
    }

    #[test]
    fn test_empty_body() {
        assert!(parse_wikilink_titles("").is_empty());
    }

    #[test]
    fn test_whitespace_trimmed() {
        let titles = parse_wikilink_titles("[[ Rust Language ]]");
        assert_eq!(titles, vec!["Rust Language"]);
    }

    #[test]
    fn test_empty_target_skipped() {
        // [[]] and [[  ]] should produce no titles.
        let titles = parse_wikilink_titles("[[]] and [[  ]]");
        assert!(titles.is_empty());
    }

    #[test]
    fn test_empty_pipe_target_skipped() {
        // [[|display]] — target is empty, should be skipped.
        let titles = parse_wikilink_titles("[[|display text]]");
        assert!(titles.is_empty());
    }

    #[test]
    fn test_duplicate_titles_preserved() {
        let titles = parse_wikilink_titles("[[Rust]] and [[Rust]]");
        assert_eq!(titles.len(), 2);
        assert_eq!(titles[0], "Rust");
        assert_eq!(titles[1], "Rust");
    }

    #[test]
    fn test_adjacent_wikilinks() {
        let titles = parse_wikilink_titles("[[A]][[B]][[C]]");
        assert_eq!(titles, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_no_wikilinks() {
        assert!(parse_wikilink_titles("No links here.").is_empty());
    }
}

/// The distinct, lowercased titles a body links to — the form the server
/// resolves (wiki-link matching is case-insensitive). Order of first use.
pub fn linked_titles_lowered(content: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    parse_wikilink_titles(content)
        .into_iter()
        .map(|t| t.to_lowercase())
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

#[cfg(test)]
mod lowered_tests {
    use super::linked_titles_lowered;

    #[test]
    fn lowered_titles_are_distinct_and_case_folded() {
        assert_eq!(
            linked_titles_lowered("[[Rust]] and [[rust|r]] and [[ Go ]] and [[Rust Language]]"),
            vec!["rust", "go", "rust language"]
        );
        assert!(linked_titles_lowered("no links").is_empty());
    }
}
