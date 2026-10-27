//! Node-editor save lifecycle (#161): the header's save-state label, the
//! stall watchdog's thresholds and bookkeeping, and the decision whether a
//! locally kept edit draft is worth offering back.
//!
//! The editor used to show "Saving…" for as long as a request stayed open,
//! which for a hung `fetch` is forever, and a 401 refresh reload threw the
//! in-flight edit away. Everything here is pure so the rules are unit-tested
//! on the host; the ui crate wires them to timers, signals and localStorage.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A save still open after this long reads "Still saving…".
pub const STILL_SAVING_AFTER_MS: u32 = 5_000;

/// A save still open after this long is given up on: the label becomes
/// "Save failed" with Retry, and saving is re-enabled. Half the API's 30 s
/// request timeout, and far past a healthy save (well under a second).
pub const SAVE_FAILED_AFTER_MS: u32 = 15_000;

/// Save lifecycle, surfaced in the editor header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState {
    /// Pristine: nothing to persist.
    Idle,
    /// Local changes not yet persisted (autosave pending).
    Dirty,
    /// A save is in flight.
    Saving,
    /// A save has been in flight past [`STILL_SAVING_AFTER_MS`].
    StillSaving,
    /// Persisted: server-side in edit mode, localStorage draft in create mode.
    Saved,
    /// The last save failed or was given up on; the edits are still held
    /// locally (and, in edit mode, in a local draft).
    Failed,
}

impl SaveState {
    /// Indicator label. Create-mode wording must not imply the node exists
    /// on the server, only a local draft does. `saved_at` (local time of the
    /// last server save) turns "Saved" into "Saved · 4:02 PM", so a glance
    /// answers "did my last keystrokes land?" (#98 item 6).
    pub fn label(self, edit_mode: bool, saved_at: Option<&str>) -> String {
        match (self, saved_at) {
            (SaveState::Idle, _) => String::new(),
            (SaveState::Dirty, _) => "Unsaved changes\u{2026}".into(),
            (SaveState::Saving, _) => "Saving\u{2026}".into(),
            (SaveState::StillSaving, _) => "Still saving\u{2026}".into(),
            (SaveState::Saved, Some(at)) if edit_mode => format!("Saved \u{b7} {at}"),
            (SaveState::Saved, None) if edit_mode => "Saved".into(),
            (SaveState::Saved, _) => "Draft kept locally".into(),
            (SaveState::Failed, _) => "Save failed".into(),
        }
    }

    /// Pending states read a shade stronger than the settled "Saved".
    pub fn is_pending(self) -> bool {
        matches!(
            self,
            SaveState::Dirty | SaveState::Saving | SaveState::StillSaving
        )
    }
}

/// Which attempt a settled response belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// The attempt the indicator is waiting on.
    Watched,
    /// An attempt the watchdog gave up on, or one a newer attempt replaced.
    /// Its outcome must not drive the indicator, but a success still tells
    /// us what the server now holds.
    Late,
}

/// Numbers save attempts so the watchdog's timers and late responses act
/// only on the attempt they belong to. At most one attempt is *watched*
/// (drives the indicator, blocks a second concurrent save); abandoned ones
/// may still be in flight.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SaveTracker {
    last: u64,
    watched: Option<u64>,
}

impl SaveTracker {
    /// Start a new attempt; it becomes the watched one.
    pub fn begin(&mut self) -> u64 {
        self.last += 1;
        self.watched = Some(self.last);
        self.last
    }

    /// Whether a save is being waited on (the Save button and the autosave
    /// executor hold off while it is).
    pub fn in_flight(&self) -> bool {
        self.watched.is_some()
    }

    /// Whether `id` is still the watched attempt: the "Still saving…" timer
    /// only relabels the attempt it was started for.
    pub fn is_watched(&self, id: u64) -> bool {
        self.watched == Some(id)
    }

    /// The give-up timer for `id` fired. Returns `true` when `id` was still
    /// watched, i.e. the indicator should flip to [`SaveState::Failed`]; it
    /// is then no longer watched, so saving is re-enabled.
    pub fn give_up(&mut self, id: u64) -> bool {
        if self.is_watched(id) {
            self.watched = None;
            true
        } else {
            false
        }
    }

    /// A response for `id` arrived.
    pub fn settle(&mut self, id: u64) -> Settled {
        if self.is_watched(id) {
            self.watched = None;
            Settled::Watched
        } else {
            Settled::Late
        }
    }
}

/// An unsaved edit-mode change kept on this device until the server
/// confirms it. Keyed by user and node in the ui's draft store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EditDraft {
    pub title: String,
    pub body: String,
    pub status: String,
    /// When the draft was last written (shown in the restore notice).
    pub kept_at: DateTime<Utc>,
    /// [`fingerprint`] of the server copy the edits were made on top of, so
    /// a restore can warn when the node changed elsewhere since.
    pub base: u64,
}

impl EditDraft {
    /// Parse a draft from its stored JSON form. Corrupt or legacy payloads
    /// yield `None` (treated as "no draft").
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }

    /// Whether the draft holds exactly `(title, body, status)`.
    pub fn holds(&self, title: &str, body: &str, status: &str) -> bool {
        self.title == title && self.body == body && self.status == status
    }
}

/// What to do with a kept draft when the editor reopens its node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftOnReturn {
    /// The server already has the draft's text (the save landed after all):
    /// delete it without asking.
    Discard,
    /// Offer Restore / Discard. `server_changed` is set when the server copy
    /// is no longer the one the edits were based on, so restoring would
    /// replace someone's newer text (it stays in History).
    Offer { server_changed: bool },
}

/// Decide what to do with `draft` given the server's current
/// `(title, body, status)`.
pub fn on_return(draft: &EditDraft, title: &str, body: &str, status: &str) -> DraftOnReturn {
    if draft.holds(title, body, status) {
        DraftOnReturn::Discard
    } else {
        DraftOnReturn::Offer {
            server_changed: fingerprint(title, body, status) != draft.base,
        }
    }
}

/// Stable 64-bit FNV-1a fingerprint of a node snapshot. Not a security
/// boundary, only a "did the server copy change?" check; stable across
/// builds and platforms (unlike `DefaultHasher`), since drafts outlive the
/// bundle that wrote them.
pub fn fingerprint(title: &str, body: &str, status: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    // Unit separators keep ("ab", "c") and ("a", "bc") apart.
    [
        title.as_bytes(),
        b"\x1f",
        body.as_bytes(),
        b"\x1f",
        status.as_bytes(),
    ]
    .iter()
    .flat_map(|part| part.iter())
    .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_label_distinguishes_modes() {
        // Create mode persists only a local draft: the label must not claim
        // the node was saved to the server.
        assert_eq!(SaveState::Saved.label(true, None), "Saved");
        assert_eq!(SaveState::Saved.label(false, None), "Draft kept locally");
        assert_eq!(SaveState::Idle.label(true, None), "");
    }

    #[test]
    fn saved_label_carries_the_save_time() {
        assert_eq!(
            SaveState::Saved.label(true, Some("4:02 PM")),
            "Saved \u{b7} 4:02 PM"
        );
        // A local draft never claims a server save time.
        assert_eq!(
            SaveState::Saved.label(false, Some("4:02 PM")),
            "Draft kept locally"
        );
        assert_eq!(
            SaveState::Dirty.label(true, Some("4:02 PM")),
            "Unsaved changes\u{2026}"
        );
    }

    #[test]
    fn stall_labels_escalate() {
        assert_eq!(SaveState::Saving.label(true, None), "Saving\u{2026}");
        assert_eq!(
            SaveState::StillSaving.label(true, None),
            "Still saving\u{2026}"
        );
        assert_eq!(
            SaveState::Failed.label(true, Some("4:02 PM")),
            "Save failed"
        );
        assert!(SaveState::StillSaving.is_pending());
        assert!(!SaveState::Failed.is_pending());
        assert!(!SaveState::Saved.is_pending());
    }

    #[test]
    fn thresholds_are_ordered_and_inside_the_server_timeout() {
        const { assert!(STILL_SAVING_AFTER_MS < SAVE_FAILED_AFTER_MS) };
        // The API's TimeoutLayer answers 408 at 30 s; giving up earlier
        // means the user never waits on it.
        const { assert!(SAVE_FAILED_AFTER_MS < 30_000) };
    }

    #[test]
    fn healthy_attempt_settles_as_watched() {
        let mut t = SaveTracker::default();
        let a = t.begin();
        assert!(t.in_flight());
        assert_eq!(t.settle(a), Settled::Watched);
        assert!(!t.in_flight());
        // Its timers fire later and must do nothing.
        assert!(!t.is_watched(a));
        assert!(!t.give_up(a));
    }

    #[test]
    fn give_up_releases_the_lock_and_a_late_response_is_late() {
        let mut t = SaveTracker::default();
        let a = t.begin();
        assert!(t.is_watched(a), "still-saving timer relabels it");
        assert!(t.give_up(a), "the indicator flips to failed");
        assert!(!t.in_flight(), "saving is re-enabled");
        assert_eq!(t.settle(a), Settled::Late);
    }

    #[test]
    fn a_retry_supersedes_the_abandoned_attempt() {
        let mut t = SaveTracker::default();
        let a = t.begin();
        assert!(t.give_up(a));
        let b = t.begin();
        assert_ne!(a, b);
        // The abandoned request lands first: it must not settle the retry.
        assert_eq!(t.settle(a), Settled::Late);
        assert!(t.in_flight());
        assert!(t.is_watched(b));
        // Nor may a stale give-up timer fail the retry.
        assert!(!t.give_up(a));
        assert_eq!(t.settle(b), Settled::Watched);
    }

    fn draft(title: &str, body: &str, base: u64) -> EditDraft {
        EditDraft {
            title: title.into(),
            body: body.into(),
            status: "draft".into(),
            kept_at: DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
            base,
        }
    }

    #[test]
    fn draft_parse_round_trips_and_rejects_corrupt_json() {
        let d = draft("T", "body\nline", 42);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(EditDraft::parse(&json), Some(d));
        assert_eq!(EditDraft::parse("{not json"), None);
        assert_eq!(EditDraft::parse(r#"{"title":"x"}"#), None);
    }

    #[test]
    fn a_draft_the_server_already_has_is_discarded_silently() {
        // Sighting 2: the save landed but the tab never heard back.
        let base = fingerprint("T", "old", "draft");
        let d = draft("T", "new", base);
        assert_eq!(on_return(&d, "T", "new", "draft"), DraftOnReturn::Discard);
    }

    #[test]
    fn unsaved_edits_on_an_unchanged_node_are_offered() {
        // Sighting 1: the edit never reached the server.
        let base = fingerprint("T", "old", "draft");
        let d = draft("T", "new", base);
        assert_eq!(
            on_return(&d, "T", "old", "draft"),
            DraftOnReturn::Offer {
                server_changed: false
            }
        );
    }

    #[test]
    fn a_node_changed_elsewhere_is_flagged() {
        let base = fingerprint("T", "old", "draft");
        let d = draft("T", "new", base);
        assert_eq!(
            on_return(&d, "T", "edited on another device", "draft"),
            DraftOnReturn::Offer {
                server_changed: true
            }
        );
        // A status-only change elsewhere counts too.
        assert_eq!(
            on_return(&d, "T", "old", "published"),
            DraftOnReturn::Offer {
                server_changed: true
            }
        );
    }

    #[test]
    fn fingerprint_is_stable_and_field_separated() {
        // Pinned: drafts written by one build are read by the next.
        assert_eq!(fingerprint("", "", ""), 0x0879_e907_b528_1763);
        assert_eq!(fingerprint("T", "old", "draft"), 0xf78e_de3b_361f_7ac1);
        assert_ne!(fingerprint("ab", "c", "d"), fingerprint("a", "bc", "d"));
        assert_eq!(fingerprint("a", "b", "c"), fingerprint("a", "b", "c"));
    }
}
