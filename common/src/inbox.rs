//! DTOs for the quick-capture endpoint (`POST /api/inbox/quick`).
//!
//! Quick capture lands one capture in the user's Inbox (ONTOLOGY §9.1,
//! #234; before 3.5.0 it made a task with no Project). The path and wire
//! format stay as they were: installed service workers and the iOS Web
//! Share Target call it, sending `title`, `text` and `url`, which the
//! server joins into one capture. Processing happens later, in the Inbox.

use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::CaptureId;
use crate::task::TaskPriority;

/// The longest a capture may be (`capture::MAX_LEN`). It was 500 while
/// quick capture made a task title, which cut a shared article's text.
pub const QUICK_CAPTURE_MAX_LEN: usize = crate::capture::MAX_LEN;

/// The most each of `title` and `body` may carry, in characters. Larger
/// than one capture on purpose: text past [`QUICK_CAPTURE_MAX_LEN`] is
/// clipped and reported (`truncated`), not refused, so a long share still
/// lands. It only bounds the request.
pub const QUICK_CAPTURE_FIELD_MAX: usize = 4 * QUICK_CAPTURE_MAX_LEN;

/// Request body for `POST /api/inbox/quick`.
///
/// Either `title` or `body` must be non-empty (validated server-side).  The
/// server concatenates them with a newline if both are present, then
/// truncates the result to `QUICK_CAPTURE_MAX_LEN` chars (Unicode-safe), the
/// most one capture holds. Limits count characters, as the database does.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct QuickCaptureRequest {
    #[garde(length(chars, max = QUICK_CAPTURE_FIELD_MAX))]
    #[serde(default)]
    pub title: Option<String>,

    /// Optional supplementary text (URL, body of a shared web page, free notes).
    #[garde(length(chars, max = QUICK_CAPTURE_FIELD_MAX))]
    #[serde(default)]
    pub body: Option<String>,

    /// Sent by clients from before 3.5.0, whose capture box parsed a due
    /// date. A capture has no date, so it is written into the text
    /// ([`capture_text`]) rather than dropped.
    #[garde(skip)]
    #[serde(default)]
    pub due_date: Option<chrono::NaiveDate>,

    /// Sent by clients from before 3.5.0. A capture isn't classified yet,
    /// so it has no priority: accepted and ignored.
    #[garde(skip)]
    #[serde(default)]
    pub priority: Option<TaskPriority>,
}

/// Response body for `POST /api/inbox/quick`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuickCaptureResponse {
    pub id: CaptureId,
    /// `true` when the input was longer than `QUICK_CAPTURE_MAX_LEN` and was
    /// shortened on the way in.  The UI can surface this as "Captured —
    /// edit to add full body" so the user knows nothing was silently lost.
    pub truncated: bool,
}

/// Build a capture's text from a `(title, body)` pair.
///
/// Rules:
/// * Trim each part.
/// * If both are non-empty, join with `"\n"`.
/// * Replace internal control chars (other than `\n`) with a single space so
///   pasted clipboard junk (NULs, BELs) doesn't sneak into the DB.
/// * Truncate to `QUICK_CAPTURE_MAX_LEN` *characters* (not bytes), preserving
///   UTF-8 boundaries.
///
/// Returns `(combined, truncated)`.  `combined` is empty only when both
/// inputs were empty/whitespace; the handler must reject that case.
#[must_use]
pub fn coalesce_capture(title: Option<&str>, body: Option<&str>) -> (String, bool) {
    let t = title.map(str::trim).unwrap_or_default();
    let b = body.map(str::trim).unwrap_or_default();

    let mut combined = match (t.is_empty(), b.is_empty()) {
        (true, true) => String::new(),
        (false, true) => t.to_string(),
        (true, false) => b.to_string(),
        (false, false) => format!("{t}\n{b}"),
    };

    // Strip control chars except newline.  Avoids smuggling NULs etc. into
    // the DB where they later trip terminal renderers / tooling.
    combined = combined
        .chars()
        .map(|c| if c == '\n' || !c.is_control() { c } else { ' ' })
        .collect();

    let char_count = combined.chars().count();
    if char_count <= QUICK_CAPTURE_MAX_LEN {
        return (combined, false);
    }
    // Take exactly QUICK_CAPTURE_MAX_LEN chars (UTF-8 safe).  Append " …"
    // marker so the user sees at a glance that it was clipped.
    let truncated: String = combined.chars().take(QUICK_CAPTURE_MAX_LEN - 2).collect();
    (format!("{truncated} …"), true)
}

/// What a quick capture saves (#234): its title and body joined
/// ([`coalesce_capture`]), led by an old client's due date, since a capture
/// has no date of its own. Empty when there is no text at all, whatever
/// the date: the handler refuses that.
#[must_use]
pub fn capture_text(req: &QuickCaptureRequest) -> (String, bool) {
    let (text, _) = coalesce_capture(req.title.as_deref(), req.body.as_deref());
    if text.is_empty() {
        return (text, false);
    }
    let title = match req.due_date {
        Some(due) => Some(format!(
            "Due {}: {}",
            due.format("%a %-d %b"),
            req.title.as_deref().unwrap_or("").trim()
        )),
        None => req.title.clone(),
    };
    coalesce_capture(title.as_deref(), req.body.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(title: &str, body: &str, due: Option<(i32, u32, u32)>) -> QuickCaptureRequest {
        QuickCaptureRequest {
            title: Some(title.into()),
            body: Some(body.into()),
            due_date: due.and_then(|(y, m, d)| chrono::NaiveDate::from_ymd_opt(y, m, d)),
            priority: Some(TaskPriority::High),
        }
    }

    #[test]
    fn an_old_clients_due_date_leads_the_capture_and_priority_is_ignored() {
        assert_eq!(
            capture_text(&req("call the bank", "", Some((2026, 11, 7)))).0,
            "Due Sat 7 Nov: call the bank"
        );
        assert_eq!(
            capture_text(&req("call the bank", "", None)).0,
            "call the bank"
        );
        // A date alone is no capture.
        assert_eq!(capture_text(&req(" ", "", Some((2026, 11, 7)))).0, "");
    }

    #[test]
    fn a_shared_article_keeps_far_more_than_a_task_title_did() {
        let text = "x".repeat(4_000);
        let (out, truncated) = capture_text(&req("Article", &text, None));
        assert!(!truncated);
        assert_eq!(out.chars().count(), "Article\n".len() + 4_000);
    }

    #[test]
    fn long_or_multibyte_shares_are_clipped_not_refused() {
        // A share of 3 000 Cyrillic characters (6 000 bytes) is accepted.
        let cyrillic = req("", &"ж".repeat(3_000), None);
        assert!(cyrillic.validate().is_ok());
        assert!(!capture_text(&cyrillic).1);

        // Text past one capture's length is accepted and clipped, so
        // `truncated` can tell the user, rather than a 422.
        let long = req("", &"x".repeat(QUICK_CAPTURE_MAX_LEN + 2_000), None);
        assert!(long.validate().is_ok());
        let (out, truncated) = capture_text(&long);
        assert!(truncated);
        assert_eq!(out.chars().count(), QUICK_CAPTURE_MAX_LEN);

        // Still bounded.
        let huge = req("", &"x".repeat(QUICK_CAPTURE_FIELD_MAX + 1), None);
        assert!(huge.validate().is_err());
    }

    #[test]
    fn coalesce_returns_empty_when_both_blank() {
        let (out, trunc) = coalesce_capture(None, None);
        assert_eq!(out, "");
        assert!(!trunc);

        let (out, _) = coalesce_capture(Some("   "), Some("\t\n"));
        assert_eq!(out, "");
    }

    #[test]
    fn coalesce_uses_title_when_body_blank() {
        let (out, trunc) = coalesce_capture(Some("buy milk"), None);
        assert_eq!(out, "buy milk");
        assert!(!trunc);
    }

    #[test]
    fn coalesce_uses_body_when_title_blank() {
        let (out, _) = coalesce_capture(Some(""), Some("https://example.com"));
        assert_eq!(out, "https://example.com");
    }

    #[test]
    fn coalesce_joins_both_with_newline() {
        let (out, _) = coalesce_capture(Some("Read this"), Some("https://example.com"));
        assert_eq!(out, "Read this\nhttps://example.com");
    }

    #[test]
    fn coalesce_strips_control_chars_keeps_newline() {
        let (out, _) = coalesce_capture(Some("hi\u{0007}there"), Some("ok\u{0000}"));
        assert!(!out.contains('\u{0007}'));
        assert!(!out.contains('\u{0000}'));
        assert!(out.contains('\n'));
    }

    #[test]
    fn coalesce_truncates_long_text_at_char_boundary() {
        let long = "a".repeat(QUICK_CAPTURE_MAX_LEN + 100);
        let (out, trunc) = coalesce_capture(Some(&long), None);
        assert!(trunc);
        assert_eq!(out.chars().count(), QUICK_CAPTURE_MAX_LEN);
        assert!(out.ends_with(" …"));
    }

    #[test]
    fn coalesce_truncation_is_unicode_safe() {
        // Each emoji is ≥1 char; ensure we never split a grapheme into
        // invalid UTF-8.  500 emojis exceeds the limit.
        let text: String = "😀".repeat(QUICK_CAPTURE_MAX_LEN + 50);
        let (out, trunc) = coalesce_capture(Some(&text), None);
        assert!(trunc);
        // String must remain valid UTF-8 (the assertion is implicit — if
        // truncation split a char this test would not compile/run cleanly).
        assert_eq!(out.chars().count(), QUICK_CAPTURE_MAX_LEN);
    }
}
