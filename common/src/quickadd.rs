//! Natural-language token parsing for quick capture.
//!
//! `"buy milk friday p1"` → title `"buy milk"`, due next Friday, high
//! priority. Only the **first line** of the input is scanned — shared URLs
//! or pasted text on later lines are never rewritten. Tokens are plain
//! whitespace-separated words (case-insensitive); when the same category
//! appears twice the last occurrence wins; every recognized token is removed
//! from the title. If stripping would leave the first line empty, the input
//! is returned untouched — `"tomorrow"` alone is a title, not a date.
//!
//! Parsing is intentionally client-side and pure: the UI previews the
//! interpretation live (chips under the capture box), so misfires like a
//! literal "p1" in prose are visible before submitting.

use chrono::{Datelike, NaiveDate, TimeDelta, Weekday};

use crate::task::TaskPriority;

/// Result of parsing a quick-capture input.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickAddParse {
    /// Input with recognized tokens removed from the first line; remaining
    /// lines are preserved verbatim.
    pub title: String,
    pub due_date: Option<NaiveDate>,
    pub priority: Option<TaskPriority>,
}

/// Parse date/priority tokens out of `input`. `today` anchors relative dates
/// (pass the user's local date; injected for determinism).
#[must_use]
pub fn parse_quick_add(input: &str, today: NaiveDate) -> QuickAddParse {
    let unparsed = || QuickAddParse {
        title: input.trim().to_string(),
        due_date: None,
        priority: None,
    };

    let (first_line, rest) = match input.split_once('\n') {
        Some((f, r)) => (f, Some(r)),
        None => (input, None),
    };

    let mut due: Option<NaiveDate> = None;
    let mut priority: Option<TaskPriority> = None;
    let mut kept: Vec<&str> = Vec::new();

    for word in first_line.split_whitespace() {
        if let Some(p) = parse_priority_token(word) {
            priority = Some(p);
        } else if let Some(d) = parse_date_token(word, today) {
            due = Some(d);
        } else {
            kept.push(word);
        }
    }

    if due.is_none() && priority.is_none() {
        return unparsed();
    }
    if kept.is_empty() {
        // Tokens were the whole line — treat them as the title instead.
        return unparsed();
    }

    let mut title = kept.join(" ");
    if let Some(rest) = rest {
        title.push('\n');
        title.push_str(rest);
    }
    QuickAddParse {
        title: title.trim().to_string(),
        due_date: due,
        priority,
    }
}

fn parse_priority_token(word: &str) -> Option<TaskPriority> {
    match word.to_ascii_lowercase().as_str() {
        "p1" | "!high" => Some(TaskPriority::High),
        "p2" | "!medium" | "!med" => Some(TaskPriority::Medium),
        "p3" | "!low" => Some(TaskPriority::Low),
        _ => None,
    }
}

fn parse_date_token(word: &str, today: NaiveDate) -> Option<NaiveDate> {
    let lower = word.to_ascii_lowercase();
    match lower.as_str() {
        "today" | "tod" => return Some(today),
        "tomorrow" | "tmrw" | "tmr" => return Some(today + TimeDelta::days(1)),
        _ => {}
    }
    if let Some(target) = parse_weekday(&lower) {
        // Upcoming occurrence; the named day on that same day means today
        // ("friday", said on a Friday, is still this Friday).
        let ahead = (target.num_days_from_monday() as i64
            - today.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        return Some(today + TimeDelta::days(ahead));
    }
    // ISO date — only the unambiguous YYYY-MM-DD form.
    if lower.len() == 10 && lower.as_bytes()[4] == b'-' && lower.as_bytes()[7] == b'-' {
        return lower.parse::<NaiveDate>().ok();
    }
    None
}

fn parse_weekday(word: &str) -> Option<Weekday> {
    match word {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thur" | "thurs" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-06-10 is a Wednesday.
    fn wed() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 10).expect("valid date")
    }

    #[test]
    fn parses_tomorrow_and_p1() {
        let p = parse_quick_add("buy milk tomorrow p1", wed());
        assert_eq!(p.title, "buy milk");
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 6, 11));
        assert_eq!(p.priority, Some(TaskPriority::High));
    }

    #[test]
    fn weekday_resolves_to_upcoming_occurrence() {
        let p = parse_quick_add("call mom friday", wed());
        assert_eq!(p.title, "call mom");
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 6, 12));
        // Monday from a Wednesday wraps to next week.
        let p = parse_quick_add("review monday", wed());
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 6, 15));
        // The named day on that same day means today.
        let p = parse_quick_add("standup wednesday", wed());
        assert_eq!(p.due_date, Some(wed()));
    }

    #[test]
    fn parses_iso_date_and_bang_priority() {
        let p = parse_quick_add("ship release 2026-07-01 !low", wed());
        assert_eq!(p.title, "ship release");
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 7, 1));
        assert_eq!(p.priority, Some(TaskPriority::Low));
    }

    #[test]
    fn last_token_wins_per_category() {
        let p = parse_quick_add("pay rent today tomorrow", wed());
        assert_eq!(p.title, "pay rent");
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 6, 11));
    }

    #[test]
    fn token_only_input_stays_a_title() {
        let p = parse_quick_add("tomorrow", wed());
        assert_eq!(p.title, "tomorrow");
        assert_eq!(p.due_date, None);
        let p = parse_quick_add("p1 friday", wed());
        assert_eq!(p.title, "p1 friday");
        assert_eq!(p.priority, None);
    }

    #[test]
    fn only_first_line_is_scanned() {
        let input = "read this friday\nhttps://example.com p1";
        let p = parse_quick_add(input, wed());
        assert_eq!(p.title, "read this\nhttps://example.com p1");
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 6, 12));
        assert_eq!(
            p.priority, None,
            "p1 on a later line is content, not a token"
        );
    }

    #[test]
    fn no_tokens_returns_input_unchanged() {
        let p = parse_quick_add("just a plain thought", wed());
        assert_eq!(p.title, "just a plain thought");
        assert_eq!(p.due_date, None);
        assert_eq!(p.priority, None);
    }

    #[test]
    fn tokens_are_case_insensitive() {
        let p = parse_quick_add("Email team Friday P1", wed());
        assert_eq!(p.title, "Email team");
        assert_eq!(p.due_date, NaiveDate::from_ymd_opt(2026, 6, 12));
        assert_eq!(p.priority, Some(TaskPriority::High));
    }
}
