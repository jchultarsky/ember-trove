//! Editor draft persistence via `localStorage`.
//!
//! Losing a half-written node to a stray navigation is the worst editor
//! failure mode. Create mode has nothing to PATCH yet, so its draft is kept
//! locally: written (debounced) on every change, restored on the next visit
//! to `/nodes/new`, and cleared on successful create.
//!
//! Edit mode autosaves, but a save can stall, fail, or be cut off by a
//! reload (the 401 refresh path reloads the page, #161). So its unconfirmed
//! changes are kept too, per user and node, from the moment a save starts
//! until the server confirms them; the editor offers them back on return
//! (`common::save_status::on_return`). An explicit logout clears every draft,
//! so personal content doesn't outlive the session on a shared device.
//!
//! All operations are infallible — a missing `localStorage` (private
//! browsing, SSR) degrades to "no draft", same as `recent.rs`.

use serde::{Deserialize, Serialize};

const LS_KEY: &str = "ember_trove_create_draft";
/// Prefix of the per-user, per-node edit-draft keys.
const EDIT_PREFIX: &str = "ember_trove_edit_draft:";

/// Storage key of `subject`'s edit draft for `node`. The subject is part of
/// the key so a second account in the same browser is never offered another
/// user's unsaved text of a node shared with both.
fn edit_key(subject: &str, node: &str) -> String {
    format!("{EDIT_PREFIX}{subject}:{node}")
}

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
    let raw = storage()?.get_item(LS_KEY).ok().flatten()?;
    CreateDraft::parse(&raw)
}

/// Persist the draft. Silently no-ops on any storage error.
pub fn write_draft(draft: &CreateDraft) {
    if let (Some(storage), Ok(json)) = (storage(), serde_json::to_string(draft)) {
        let _ = storage.set_item(LS_KEY, &json);
    }
}

/// Remove the persisted draft (after a successful create, or when the
/// editor content reverts to pristine).
pub fn clear_draft() {
    if let Some(storage) = storage() {
        let _ = storage.remove_item(LS_KEY);
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Read `subject`'s kept edits for `node`, if any.
pub fn read_edit_draft(subject: &str, node: &str) -> Option<common::save_status::EditDraft> {
    let raw = storage()?
        .get_item(&edit_key(subject, node))
        .ok()
        .flatten()?;
    common::save_status::EditDraft::parse(&raw)
}

/// Keep `subject`'s unconfirmed edits for `node`. Silently no-ops on any
/// storage error (a full quota included).
pub fn write_edit_draft(subject: &str, node: &str, draft: &common::save_status::EditDraft) {
    if let (Some(storage), Ok(json)) = (storage(), serde_json::to_string(draft)) {
        let _ = storage.set_item(&edit_key(subject, node), &json);
    }
}

/// Drop `subject`'s kept edits for `node` (confirmed by the server, or
/// discarded by the user).
pub fn clear_edit_draft(subject: &str, node: &str) {
    if let Some(storage) = storage() {
        let _ = storage.remove_item(&edit_key(subject, node));
    }
}

/// Remove every editor draft — the create draft and all edit drafts. Called
/// on explicit logout: the user is leaving this device, and drafts hold
/// node content in plaintext. (A session-expiry redirect keeps them; that is
/// exactly the case they exist for.)
pub fn clear_all_drafts() {
    let Some(storage) = storage() else {
        return;
    };
    let len = storage.length().unwrap_or(0);
    // Collect first: removing while indexing would skip keys.
    let doomed: Vec<String> = (0..len)
        .filter_map(|i| storage.key(i).ok().flatten())
        .filter(|k| k == LS_KEY || k.starts_with(EDIT_PREFIX))
        .collect();
    for key in doomed {
        let _ = storage.remove_item(&key);
    }
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
    fn edit_keys_are_scoped_to_user_and_node() {
        assert_eq!(edit_key("sub-a", "n1"), "ember_trove_edit_draft:sub-a:n1");
        assert_ne!(edit_key("sub-a", "n1"), edit_key("sub-b", "n1"));
        assert!(edit_key("x", "y").starts_with(EDIT_PREFIX));
        assert!(!LS_KEY.starts_with(EDIT_PREFIX));
    }

    #[test]
    fn edited_body_is_meaningful() {
        let scaffold = "## Status\n";
        assert!(draft("", "## Status\n\nactual work").is_meaningful(scaffold));
    }
}
