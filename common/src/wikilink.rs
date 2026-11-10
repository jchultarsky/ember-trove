//! Wiki-link (`[[title]]`) parsing, shared by the API (edge sync, title
//! resolution) and the UI (rendering) so both read links the same way
//! (#134; it used to live in the API only).

use std::ops::Range;

/// One `[[target]]` or `[[target|display]]` in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink<'a> {
    /// Byte range of the whole `[[…]]` in the source.
    pub range: Range<usize>,
    /// The node's title: the part before the first `|`, trimmed. Never empty.
    pub target: &'a str,
    /// What is shown: the part after the first `|`, trimmed, or the target
    /// when there is no pipe or nothing after it. Never empty.
    pub display: &'a str,
}

/// Every wiki link in `text`, in order. The one scanner (#281, 3.1): the
/// API's edge sync and title resolution, the renderer, the excerpts, the
/// plain-text previews and "link this mention" all read links through it.
/// There were five hand-written scanners, and they disagreed.
///
/// The rules:
/// - `[[target]]` or `[[target|display]]`. Both parts are trimmed. Only
///   the first `|` separates; later ones belong to the display.
/// - A link sits on one line. A `[[` with no `]]` before the line ends is
///   plain text, and scanning goes on after it.
/// - An empty target (`[[]]`, `[[ |x]]`) is plain text, not a link.
/// - An empty display (`[[x|]]`) shows the target.
/// - `[[a [[b]]` links `b`: an opener inside an opener restarts the link.
/// - Nothing inside code is a link: an inline code span or a fenced code
///   block is read as written ([`code_ranges`]; #281, 1.10).
#[must_use]
pub fn wikilinks(text: &str) -> Vec<WikiLink<'_>> {
    let code = code_ranges(text);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(rel) = text.get(from..).and_then(|rest| rest.find("[[")) {
        let open = from + rel;
        let inner_start = open + 2;
        if in_code(open) {
            from = inner_start;
            continue;
        }
        let rest = text.get(inner_start..).unwrap_or_default();
        let line = rest.split('\n').next().unwrap_or_default();
        let Some(close_rel) = line.find("]]") else {
            from = inner_start;
            continue;
        };
        let inner = line.get(..close_rel).unwrap_or_default();
        // `[[a [[b]]`: the inner opener starts the link.
        if let Some(nested) = inner.rfind("[[") {
            from = inner_start + nested;
            continue;
        }
        let end = inner_start + close_rel + 2;
        let (target, display) = match inner.split_once('|') {
            Some((target, display)) => (target.trim(), display.trim()),
            None => (inner.trim(), ""),
        };
        if !target.is_empty() {
            links.push(WikiLink {
                range: open..end,
                target,
                display: if display.is_empty() { target } else { display },
            });
        }
        from = end;
    }
    links
}

/// `text` with each wiki link replaced by what `shown` returns for it.
/// Everything else, code included, is copied as it is.
#[must_use]
pub fn replace_wikilinks(text: &str, mut shown: impl FnMut(&WikiLink<'_>) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for link in wikilinks(text) {
        out.push_str(text.get(at..link.range.start).unwrap_or_default());
        out.push_str(&shown(&link));
        at = link.range.end;
    }
    out.push_str(text.get(at..).unwrap_or_default());
    out
}

/// The byte ranges of `text` that Markdown reads as code: fenced blocks
/// (``` or ~~~, to the closing fence or the end of the text) and inline
/// code spans (a run of backticks up to the next run of the same length in
/// the same paragraph). Indented code blocks are not recognised: four
/// spaces also continue a list item, and a link there is far more likely.
#[must_use]
pub fn code_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    // Fenced blocks, line by line.
    let mut fence: Option<(char, usize, usize)> = None; // (char, length, start)
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = marker.map_or(0, |m| trimmed.chars().take_while(|c| *c == m).count());
        match (fence, marker) {
            (None, Some(m)) if indent <= 3 && run >= 3 => fence = Some((m, run, offset)),
            (Some((m, len, start)), Some(found))
                if found == m && indent <= 3 && run >= len && trimmed[run..].trim().is_empty() =>
            {
                ranges.push(start..offset + line.len());
                fence = None;
            }
            _ => {}
        }
        offset += line.len();
    }
    if let Some((_, _, start)) = fence {
        ranges.push(start..text.len());
    }
    // Inline spans, outside the fences.
    let fenced = ranges.clone();
    let in_fence = |at: usize| fenced.iter().any(|r| r.contains(&at));
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' || in_fence(i) {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let after = i + run;
        // The closing run: the same length, before the paragraph ends.
        let paragraph_end = text
            .get(after..)
            .and_then(|rest| rest.find("\n\n"))
            .map_or(text.len(), |p| after + p);
        let mut j = after;
        let mut close = None;
        while j < paragraph_end {
            if bytes[j] == b'`' {
                let len = bytes[j..].iter().take_while(|b| **b == b'`').count();
                if len == run {
                    close = Some(j + len);
                    break;
                }
                j += len;
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                ranges.push(i..end);
                i = end;
            }
            // No closing run: the backticks are plain text.
            None => i = after,
        }
    }
    ranges.sort_by_key(|r| r.start);
    ranges
}

/// Extract all wiki-link target titles from a markdown body, in order.
/// Duplicate titles are preserved; deduplication is the caller's concern.
/// See [`wikilinks`] for what counts as a link.
pub fn parse_wikilink_titles(content: &str) -> Vec<String> {
    wikilinks(content)
        .into_iter()
        .map(|link| link.target.to_string())
        .collect()
}

#[cfg(test)]
mod tokenizer_tests {
    use super::*;

    fn pairs(text: &str) -> Vec<(&str, &str)> {
        wikilinks(text)
            .into_iter()
            .map(|l| (l.target, l.display))
            .collect()
    }

    #[test]
    fn a_link_has_a_target_and_what_is_shown() {
        assert_eq!(pairs("[[Rust]]"), [("Rust", "Rust")]);
        assert_eq!(
            pairs("[[ Rust | the language ]]"),
            [("Rust", "the language")]
        );
        // Only the first pipe separates.
        assert_eq!(pairs("[[a|b|c]]"), [("a", "b|c")]);
        // Nothing after the pipe: the target is shown.
        assert_eq!(pairs("[[Rust|]]"), [("Rust", "Rust")]);
        assert_eq!(pairs("[[Rust|  ]]"), [("Rust", "Rust")]);
    }

    #[test]
    fn the_range_covers_the_brackets() {
        let text = "see [[A|b]] now";
        let link = &wikilinks(text)[0];
        assert_eq!(&text[link.range.clone()], "[[A|b]]");
    }

    #[test]
    fn no_target_is_not_a_link() {
        assert!(pairs("[[]] and [[  ]] and [[|shown]] and [[ | ]]").is_empty());
    }

    #[test]
    fn a_link_does_not_cross_a_line() {
        // The five old scanners took everything up to the next `]]`,
        // paragraphs away, as one link.
        assert_eq!(
            pairs("an open [[ here\nand [[Real]] there"),
            [("Real", "Real")]
        );
        assert_eq!(pairs("[[A]] then [[unclosed"), [("A", "A")]);
        // Scanning goes on after an opener that never closes.
        assert_eq!(pairs("[[ nope\n[[B]]"), [("B", "B")]);
    }

    #[test]
    fn an_opener_inside_an_opener_restarts_the_link() {
        assert_eq!(pairs("[[a [[b]]"), [("b", "b")]);
    }

    #[test]
    fn nothing_inside_code_is_a_link() {
        // #281, 1.10: the renderer put an <a> inside the code, and the API
        // made an edge for it.
        assert!(pairs("use `[[Title]]` to link").is_empty());
        assert!(pairs("``a ` [[T]] b``").is_empty());
        assert!(pairs("```\n[[In a fence]]\n```").is_empty());
        assert!(pairs("~~~md\n[[In a fence]]\n~~~\n").is_empty());
        // An unclosed fence runs to the end.
        assert!(pairs("```\n[[Still code]]").is_empty());
        // Around the code, links are links.
        assert_eq!(
            pairs("[[Before]] `[[code]]` [[After]]\n```\n[[x]]\n```\n[[Last]]"),
            [("Before", "Before"), ("After", "After"), ("Last", "Last")]
        );
    }

    #[test]
    fn backticks_that_never_close_are_plain_text() {
        assert_eq!(pairs("a ` b [[Link]]"), [("Link", "Link")]);
        // A span does not reach into the next paragraph.
        assert_eq!(pairs("one ` two\n\n[[Link]] ` three"), [("Link", "Link")]);
        // Runs of different lengths don't close each other.
        assert_eq!(pairs("`` a ` [[Link]]"), [("Link", "Link")]);
    }

    #[test]
    fn replacing_keeps_everything_else() {
        let out = replace_wikilinks("a [[T|shown]] b `[[code]]` c [[U]]", |l| {
            format!("<{}>", l.display)
        });
        assert_eq!(out, "a <shown> b `[[code]]` c <U>");
        assert_eq!(replace_wikilinks("no links", |_| String::new()), "no links");
    }

    #[test]
    fn non_ascii_text_is_cut_on_character_boundaries() {
        assert_eq!(
            pairs("é [[Café|naïve]] ü `x` [[日本]]"),
            [("Café", "naïve"), ("日本", "日本")]
        );
    }
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
