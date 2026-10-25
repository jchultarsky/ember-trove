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
}
