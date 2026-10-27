//! Combobox rules for the node picker (#126), kept here so they're
//! host-tested: which option is active as the user types, when typed text
//! may resolve to a node on its own, and how the arrow keys move.
//!
//! Matching is case-insensitive on trimmed text. Filtering only ever
//! *highlights*; committing a value is the caller's job (Enter / press).

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// The option to highlight for `query` among `titles` (in result order):
/// the first exact title match, else the first prefix match, else the first
/// substring match, else the first result. `None` when there are no results.
pub fn best_index<S: AsRef<str>>(query: &str, titles: &[S]) -> Option<usize> {
    if titles.is_empty() {
        return None;
    }
    let q = norm(query);
    if q.is_empty() {
        return Some(0);
    }
    let lowered: Vec<String> = titles.iter().map(|t| norm(t.as_ref())).collect();
    lowered
        .iter()
        .position(|t| *t == q)
        .or_else(|| lowered.iter().position(|t| t.starts_with(&q)))
        .or_else(|| lowered.iter().position(|t| t.contains(&q)))
        .or(Some(0))
}

/// The one result whose title equals `query`, if exactly one does — the only
/// case where typed text may stand in for a choice. Duplicate titles are
/// real in this app, so two exact matches resolve to nothing.
pub fn unique_exact<S: AsRef<str>>(query: &str, titles: &[S]) -> Option<usize> {
    let q = norm(query);
    if q.is_empty() {
        return None;
    }
    let mut hits = titles
        .iter()
        .enumerate()
        .filter(|(_, t)| norm(t.as_ref()) == q)
        .map(|(i, _)| i);
    match (hits.next(), hits.next()) {
        (Some(i), None) => Some(i),
        _ => None,
    }
}

/// Whether `query` exactly matches two or more titles. Then the
/// highlighted option is an arbitrary pick among same-titled nodes, so Enter
/// must not commit it until the user chooses one (arrow keys or hover).
pub fn is_ambiguous<S: AsRef<str>>(query: &str, titles: &[S]) -> bool {
    let q = norm(query);
    !q.is_empty()
        && titles
            .iter()
            .filter(|t| norm(t.as_ref()) == q)
            .nth(1)
            .is_some()
}

/// Arrow-key movement over `len` options: Down from nothing lands on the
/// first, Up from nothing on the last; the ends wrap (WAI-ARIA APG).
pub fn step(active: Option<usize>, len: usize, down: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (active, down) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    })
}

/// Suggestions for a free-text field such as a membership's Role (#171):
/// up to `limit` of `choices` (kept in their order, prefix matches first)
/// that contain `query`, ignoring case. A choice spelled exactly as typed is
/// left out, so the list never offers what's already in the field, while a
/// different casing ("abc" for "ABC") is still offered, as an option to
/// choose, never as a replacement.
pub fn free_text_suggestions<S: AsRef<str>>(
    query: &str,
    choices: &[S],
    limit: usize,
) -> Vec<String> {
    let typed = query.trim();
    let q = norm(query);
    let (prefix, rest): (Vec<&str>, Vec<&str>) = choices
        .iter()
        .map(AsRef::as_ref)
        .filter(|c| c.trim() != typed && norm(c).contains(&q))
        .partition(|c| norm(c).starts_with(&q));
    prefix
        .into_iter()
        .chain(rest)
        .take(limit)
        .map(str::to_string)
        .collect()
}

/// What a free-text combobox saves: the typed text, unless the user has
/// explicitly made an option active (arrow keys) before committing. Filtering
/// never stands in for a choice; that substituted "abc" for a typed "ABC".
pub fn free_text_commit<'a>(
    typed: &'a str,
    active: Option<usize>,
    options: &'a [String],
) -> &'a str {
    active
        .and_then(|i| options.get(i))
        .map_or(typed, String::as_str)
}

/// Whether a combobox option's mouse event comes from a real pointer (#174).
///
/// Pointer users commit on `mousedown`, so focus stays in the input. An
/// accessibility "press" (VoiceOver, AXPress, `element.click()`) is a
/// synthetic `click`, sometimes preceded by a synthetic `mousedown`, and
/// synthetic events carry `detail` (the click count) 0. So an option commits
/// on a `mousedown` from a pointer, or on a `click` that isn't from one:
/// exactly once either way.
pub fn is_pointer_press(detail: i32) -> bool {
    detail > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_index_prefers_exact_then_prefix_then_substring() {
        let titles = ["A Project plan", "Project plan B", "project"];
        assert_eq!(best_index("Project", &titles), Some(2), "exact wins");
        assert_eq!(best_index("project p", &titles), Some(1), "prefix next");
        assert_eq!(best_index("plan", &titles), Some(0), "then substring");
        assert_eq!(best_index("zzz", &titles), Some(0), "else first result");
        assert_eq!(best_index("  PROJECT  ", &titles), Some(2), "trim + case");
    }

    #[test]
    fn best_index_handles_empty_inputs() {
        let none: [&str; 0] = [];
        assert_eq!(best_index("x", &none), None);
        assert_eq!(best_index("", &["a", "b"]), Some(0));
    }

    #[test]
    fn unique_exact_never_guesses_between_duplicates() {
        assert_eq!(unique_exact("Plan", &["Plans", "plan", "Other"]), Some(1));
        assert_eq!(
            unique_exact("Plan", &["Plan", "plan"]),
            None,
            "duplicate titles"
        );
        assert_eq!(unique_exact("Pla", &["Plan"]), None, "prefix is not exact");
        assert_eq!(unique_exact("   ", &["   "]), None, "blank query");
    }

    #[test]
    fn is_ambiguous_only_for_repeated_exact_titles() {
        assert!(is_ambiguous("Plan", &["plan", "Plan", "Other"]));
        assert!(!is_ambiguous("Plan", &["Plan", "Plans"]));
        assert!(!is_ambiguous("", &["", ""]));
    }

    #[test]
    fn step_wraps_and_starts_at_the_ends() {
        assert_eq!(step(None, 3, true), Some(0));
        assert_eq!(step(None, 3, false), Some(2));
        assert_eq!(step(Some(2), 3, true), Some(0));
        assert_eq!(step(Some(0), 3, false), Some(2));
        assert_eq!(step(Some(1), 3, true), Some(2));
        assert_eq!(step(Some(0), 0, true), None);
    }

    #[test]
    fn free_text_suggestions_never_hide_the_typed_spelling() {
        let roles = ["abc", "Approver", "Sponsor", "lab lead"];
        // Case-insensitive match, prefix first, the typed casing kept apart.
        assert_eq!(
            free_text_suggestions("ABC", &roles, 8),
            vec!["abc".to_string()]
        );
        assert_eq!(
            free_text_suggestions("ab", &roles, 8),
            vec!["abc".to_string(), "lab lead".to_string()]
        );
        // Typed exactly as a suggestion: nothing left to offer.
        assert!(free_text_suggestions("abc", &roles, 8).is_empty());
        assert!(free_text_suggestions(" Sponsor ", &roles, 8).is_empty());
        assert_eq!(free_text_suggestions("", &roles, 2).len(), 2, "limit");
    }

    #[test]
    fn free_text_commit_keeps_typed_text_unless_an_option_is_chosen() {
        let options = vec!["abc".to_string()];
        assert_eq!(free_text_commit("ABC", None, &options), "ABC");
        assert_eq!(free_text_commit("ABC", Some(0), &options), "abc");
        assert_eq!(free_text_commit("ABC", Some(5), &options), "ABC");
    }

    #[test]
    fn pointer_presses_have_a_click_count() {
        assert!(
            !is_pointer_press(0),
            "AX press / element.click(): commit on click"
        );
        assert!(
            is_pointer_press(1),
            "mouse: commit on mousedown, not again on click"
        );
        assert!(is_pointer_press(2), "double click");
    }
}
