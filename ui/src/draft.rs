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
//! Every draft is stamped when written and expires
//! [`common::save_watch::DRAFT_TTL_MS`] (14 days) after its last write: an
//! expired draft is never offered, and [`sweep_expired_drafts`] removes
//! them at startup, including drafts of nodes that are never reopened.
//!
//! All operations are infallible — a missing `localStorage` (private
//! browsing, SSR) degrades to "no draft", same as `recent.rs`.

use common::form_draft::PersonFormDraft;
use common::save_watch::{
    EditDraft, LEGACY_CREATE_DRAFT_KEY, Sweep, create_draft_key, edit_draft_key, is_draft_key,
    is_expired, sweep,
};
use leptos::prelude::RwSignal;
use serde::{Deserialize, Serialize};

/// Unsaved create-mode editor state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateDraft {
    pub title: String,
    pub body: String,
    pub node_type: String,
    pub status: String,
    /// When the draft was last written (ms since the epoch); stamped by
    /// [`write_draft`]. Absent in drafts from before v2.41.0.
    #[serde(default)]
    pub saved_at: Option<i64>,
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

/// Read `subject`'s create draft, if any. An expired one is removed.
pub fn read_draft(subject: &str) -> Option<CreateDraft> {
    let key = create_draft_key(subject);
    let s = storage()?;
    let d = CreateDraft::parse(&s.get_item(&key).ok().flatten()?)?;
    if is_expired(d.saved_at, now_ms()) {
        let _ = s.remove_item(&key);
        return None;
    }
    Some(d)
}

/// Persist `subject`'s create draft. Silently no-ops on any storage error,
/// and after logout (see `LOGGED_OUT`).
pub fn write_draft(subject: &str, draft: &CreateDraft) {
    if LOGGED_OUT.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let draft = CreateDraft {
        saved_at: Some(now_ms()),
        ..draft.clone()
    };
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(&draft)) {
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

/// Milliseconds since the epoch, the unit of draft stamps.
fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

/// Read this user's draft of `node_id`, if any. An expired one is removed.
pub fn read_edit_draft(subject: &str, node_id: &str) -> Option<EditDraft> {
    let key = edit_draft_key(subject, node_id);
    let s = storage()?;
    let d = EditDraft::parse(&s.get_item(&key).ok().flatten()?)?;
    if is_expired(d.saved_at, now_ms()) {
        let _ = s.remove_item(&key);
        return None;
    }
    Some(d)
}

/// Keep unsaved edits of `node_id`. Silently no-ops on any storage error.
pub fn write_edit_draft(subject: &str, node_id: &str, draft: &EditDraft) {
    if LOGGED_OUT.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let draft = EditDraft {
        saved_at: Some(now_ms()),
        ..draft.clone()
    };
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(&draft)) {
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

// ── Modal-form drafts (#170) ───────────────────────────────────────────

/// Read the draft stored under `key` (a `common::form_draft::PersonForm`
/// key), if any. An expired one is removed.
pub fn read_person_draft(key: &str) -> Option<PersonFormDraft> {
    let s = storage()?;
    let d = PersonFormDraft::parse(&s.get_item(key).ok().flatten()?)?;
    if is_expired(d.saved_at, now_ms()) {
        let _ = s.remove_item(key);
        return None;
    }
    Some(d)
}

/// Keep what a person dialog holds. Silently no-ops on any storage error,
/// and after logout.
pub fn write_person_draft(key: &str, draft: &PersonFormDraft) {
    if LOGGED_OUT.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let draft = PersonFormDraft {
        saved_at: Some(now_ms()),
        ..draft.clone()
    };
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(&draft)) {
        let _ = s.set_item(key, &json);
    }
}

/// Drop a form draft (saved, cancelled, dismissed or discarded).
pub fn clear_form_draft(key: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(key);
    }
}

/// How many dialogs that keep a draft are open, so the update banner can
/// say that reloading keeps their input (#170). Provided at the app root.
#[derive(Clone, Copy)]
pub struct OpenForms(pub RwSignal<u32>);

/// Remove expired drafts and date unstamped ones (v2.40.0), for every user
/// on this device. Run once at startup: a draft of a node nobody reopens is
/// never read, so read-time expiry alone would keep it forever.
pub fn sweep_expired_drafts() {
    let Some(s) = storage() else { return };
    let now = now_ms();
    let len = s.length().unwrap_or(0);
    // Collect first: removing while indexing shifts the keys.
    let keys: Vec<String> = (0..len)
        .filter_map(|i| s.key(i).ok().flatten())
        .filter(|k| is_draft_key(k))
        .collect();
    for k in keys {
        let Some(raw) = s.get_item(&k).ok().flatten() else {
            continue;
        };
        match sweep(&raw, now) {
            Sweep::Keep => {}
            Sweep::Expire => {
                let _ = s.remove_item(&k);
            }
            Sweep::Stamp(json) => {
                let _ = s.set_item(&k, &json);
            }
        }
    }
}

/// Remove every draft on this device (edit, create and form drafts of
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
            saved_at: None,
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
    fn a_v2_40_draft_without_a_stamp_still_parses() {
        let d = CreateDraft::parse(
            r#"{"title":"t","body":"b","node_type":"article","status":"draft"}"#,
        )
        .expect("v2.40.0 drafts stay readable");
        assert_eq!(d.saved_at, None);
        let stamped = CreateDraft {
            saved_at: Some(1_800_000_000_000),
            ..d
        };
        let json = serde_json::to_string(&stamped).expect("serialize");
        assert_eq!(CreateDraft::parse(&json), Some(stamped));
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
