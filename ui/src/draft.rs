//! Create-mode editor draft persistence via `localStorage`.
//!
//! Losing a half-written node to a stray navigation is the worst editor
//! failure mode. Edit mode is covered by autosave (the node exists, so we
//! PATCH it); create mode has nothing to PATCH yet, so the draft is kept
//! locally instead: written (debounced) on every change, restored on the
//! next visit to `/nodes/new`, and cleared on successful create. It is
//! keyed by user (`common::save_watch::create_draft_key`, #161 follow-up):
//! the old single key was shared by every account in the browser, so the
//! next person to open `/nodes/new` got someone else's text restored. That
//! legacy key is deleted, never read or migrated ([`purge_legacy_create_draft`]).
//!
//! Edit mode keeps a draft too, for the edits a save hasn't landed yet
//! (#161): written when a save starts, fails or the page unloads, removed
//! once the server has them, offered back when the node is opened again.
//! It is keyed by user and node (`common::save_watch::edit_draft_key`) and
//! wiped on logout, because it holds node content on the device. Logout
//! wipes create drafts too ([`clear_all_drafts`]).
//!
//! All operations are infallible — a missing `localStorage` (private
//! browsing, SSR) degrades to "no draft", same as `recent.rs`.

use common::save_watch::{
    EditDraft, LEGACY_CREATE_DRAFT_KEY, create_draft_key, edit_draft_key, is_draft_key,
};
use serde::{Deserialize, Serialize};

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

/// Read `subject`'s create draft, if any.
pub fn read_draft(subject: &str) -> Option<CreateDraft> {
    let raw = storage()?
        .get_item(&create_draft_key(subject))
        .ok()
        .flatten()?;
    CreateDraft::parse(&raw)
}

/// Persist `subject`'s create draft. Silently no-ops on any storage error,
/// and after logout (see `LOGGED_OUT`).
pub fn write_draft(subject: &str, draft: &CreateDraft) {
    if LOGGED_OUT.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(draft)) {
        let _ = s.set_item(&create_draft_key(subject), &json);
    }
}

/// Remove `subject`'s create draft (after a successful create, or when the
/// editor content reverts to pristine).
pub fn clear_draft(subject: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(&create_draft_key(subject));
    }
}

/// Delete the pre-#161 unscoped create draft. Run once at startup: whose
/// it was is unknowable, so it is never offered to anyone.
pub fn purge_legacy_create_draft() {
    if let Some(s) = storage() {
        let _ = s.remove_item(LEGACY_CREATE_DRAFT_KEY);
    }
}

// ── Edit-mode drafts (#161) ─────────────────────────────────────────────

/// Set by [`clear_all_edit_drafts`] at logout. The logout redirect fires the
/// editor's `beforeunload`, which would write the draft straight back, so
/// once the user has logged out this page writes no more drafts.
static LOGGED_OUT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Read this user's draft of `node_id`, if any.
pub fn read_edit_draft(subject: &str, node_id: &str) -> Option<EditDraft> {
    let raw = storage()?
        .get_item(&edit_draft_key(subject, node_id))
        .ok()
        .flatten()?;
    EditDraft::parse(&raw)
}

/// Keep unsaved edits of `node_id`. Silently no-ops on any storage error.
pub fn write_edit_draft(subject: &str, node_id: &str, draft: &EditDraft) {
    if LOGGED_OUT.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(draft)) {
        let _ = s.set_item(&edit_draft_key(subject, node_id), &json);
    }
}

/// Drop this user's draft of `node_id` (its edits reached the server, or
/// the user discarded them).
pub fn clear_edit_draft(subject: &str, node_id: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(&edit_draft_key(subject, node_id));
    }
}

/// Remove every editor draft on this device (edit and create drafts of
/// every user, and the legacy key) and stop writing new ones. Called on
/// logout so node content doesn't outlive the session in `localStorage`.
pub fn clear_all_drafts() {
    LOGGED_OUT.store(true, std::sync::atomic::Ordering::Relaxed);
    let Some(s) = storage() else { return };
    let len = s.length().unwrap_or(0);
    // Collect first: removing while indexing shifts the keys.
    let keys: Vec<String> = (0..len)
        .filter_map(|i| s.key(i).ok().flatten())
        .filter(|k| is_draft_key(k))
        .collect();
    for k in keys {
        let _ = s.remove_item(&k);
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
    fn edited_body_is_meaningful() {
        let scaffold = "## Status\n";
        assert!(draft("", "## Status\n\nactual work").is_meaningful(scaffold));
    }
}
