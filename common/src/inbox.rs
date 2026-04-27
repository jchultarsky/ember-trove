//! DTOs for the quick-capture endpoint (`POST /api/inbox/quick`).
//!
//! Quick capture lands a single low-friction task in the user's Inbox
//! (`tasks` row with `node_id IS NULL`).  It exists as a separate endpoint
//! from the normal task-create flow because:
//!
//! * the iOS Web Share Target sends multipart fields (`title`, `text`, `url`)
//!   that we want to coalesce into one short Task title server-side, and
//! * the surface area is intentionally tiny — no priority, due date, or node
//!   association.  Triage happens later in the Inbox view.

use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::TaskId;

/// Maximum length of a captured task title.  Matches `CreateTaskRequest::title`.
pub const QUICK_CAPTURE_MAX_LEN: usize = 500;

/// Request body for `POST /api/inbox/quick`.
///
/// Either `title` or `body` must be non-empty (validated server-side).  The
/// server concatenates them with a newline if both are present, then
/// truncates the result to `QUICK_CAPTURE_MAX_LEN` chars (Unicode-safe) so
/// that long shared text never blows the underlying `Task.title` budget.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct QuickCaptureRequest {
    #[garde(length(max = 5000))]
    #[serde(default)]
    pub title: Option<String>,

    /// Optional supplementary text (URL, body of a shared web page, free notes).
    #[garde(length(max = 5000))]
    #[serde(default)]
    pub body: Option<String>,
}

/// Response body for `POST /api/inbox/quick`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuickCaptureResponse {
    pub id: TaskId,
    /// `true` when the input was longer than `QUICK_CAPTURE_MAX_LEN` and was
    /// shortened on the way in.  The UI can surface this as "Captured —
    /// edit to add full body" so the user knows nothing was silently lost.
    pub truncated: bool,
}

/// Build the final task title from a `(title, body)` pair.
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

#[cfg(test)]
mod tests {
    use super::*;

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
