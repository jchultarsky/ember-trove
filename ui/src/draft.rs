//! Create-mode editor draft persistence via `localStorage`.
//!
//! Losing a half-written node to a stray navigation is the worst editor
//! failure mode. Edit mode is covered by autosave (the node exists, so we
//! PATCH it); create mode has nothing to PATCH yet, so the draft is kept
//! locally instead: written (debounced) on every change, restored on the
//! next visit to `/nodes/new`, and cleared on successful create.
//!
//! All operations are infallible — a missing `localStorage` (private
//! browsing, SSR) degrades to "no draft", same as `recent.rs`.

use serde::{Deserialize, Serialize};

const LS_KEY: &str = "ember_trove_create_draft";

/// Unsaved create-mode editor state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateDraft {
    pub title: String,
    pub body: String,
    pub node_type: String,
    pub status: String,
}

impl CreateDraft {
    /// Whether the draft holds real user content worth restoring: a
    /// non-empty title, or a body that differs from the untouched type
    /// scaffold the create editor starts with. An "empty" draft is not
    /// persisted, so abandoning a pristine `/nodes/new` leaves nothing behind.
    pub fn is_meaningful(&self, type_scaffold: &str) -> bool {
        let body = self.body.trim();
        !self.title.trim().is_empty() || (!body.is_empty() && body != type_scaffold.trim())
    }

    /// Parse a draft from its stored JSON form. Corrupt or legacy payloads
    /// yield `None` (treated as "no draft").
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
}

/// Read the persisted draft, if any.
pub fn read_draft() -> Option<CreateDraft> {
    let storage = web_sys::window()?.local_storage().ok().flatten()?;
    let raw = storage.get_item(LS_KEY).ok().flatten()?;
    CreateDraft::parse(&raw)
}

/// Persist the draft. Silently no-ops on any storage error.
pub fn write_draft(draft: &CreateDraft) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    if let Ok(json) = serde_json::to_string(draft) {
        let _ = storage.set_item(LS_KEY, &json);
    }
}

/// Remove the persisted draft (after a successful create, or when the
/// editor content reverts to pristine).
pub fn clear_draft() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    let _ = storage.remove_item(LS_KEY);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str, body: &str) -> CreateDraft {
        CreateDraft {
            title: title.to_string(),
            body: body.to_string(),
            node_type: "project".to_string(),
            status: "draft".to_string(),
        }
    }

    #[test]
    fn parse_round_trips() {
        let d = draft("My node", "## Status\n\nwip");
        let json = serde_json::to_string(&d).expect("serialize");
        assert_eq!(CreateDraft::parse(&json), Some(d));
    }

    #[test]
    fn parse_rejects_corrupt_json() {
        assert_eq!(CreateDraft::parse("{not json"), None);
        assert_eq!(CreateDraft::parse(r#"{"title":"x"}"#), None); // missing fields
    }

    #[test]
    fn pristine_scaffold_is_not_meaningful() {
        let scaffold = "## Status\n\n## Notes\n";
        assert!(!draft("", scaffold).is_meaningful(scaffold));
        assert!(!draft("   ", "").is_meaningful(scaffold));
    }

    #[test]
    fn title_alone_is_meaningful() {
        assert!(draft("My node", "").is_meaningful("## Status\n"));
    }

    #[test]
    fn edited_body_is_meaningful() {
        let scaffold = "## Status\n";
        assert!(draft("", "## Status\n\nactual work").is_meaningful(scaffold));
    }
}
