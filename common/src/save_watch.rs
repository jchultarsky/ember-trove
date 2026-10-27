//! Node-editor save watchdog and edit-mode draft rules (#161).
//!
//! A save that hangs used to leave the editor on "Saving…" with no way to
//! tell saving, saved and stuck apart, and a reload in that window dropped
//! the edit. The editor now escalates a slow save to "Still saving…" and
//! then gives up on it (aborting the request) so the user gets Retry, and
//! it keeps unsaved edits of an existing node in a local draft that is
//! offered back on the next visit. The thresholds and the restore decision
//! are pure, so they live here where they are tested.

use serde::{Deserialize, Serialize};

/// After this long in flight, a save shows "Still saving…". Healthy saves
/// finish in well under a second, and the slowest one seen in #161 that
/// still landed took about two.
pub const STALL_AFTER_MS: u32 = 5_000;

/// After this long, a save is aborted and reported as failed, with Retry.
/// It is long enough that a merely slow server still lands, and short
/// enough that nobody gives up and reloads first.
pub const GIVE_UP_AFTER_MS: u32 = 15_000;

/// `localStorage` key prefix for edit-mode drafts. [`edit_draft_key`] adds
/// the user and the node, and logout removes every key with this prefix.
pub const EDIT_DRAFT_PREFIX: &str = "ember_trove_edit_draft:";

/// Storage key for one user's draft of one node. The user's subject is part
/// of the key so a second account in the same browser is never offered (or
/// shown) someone else's unsaved text, even for a node both can open.
pub fn edit_draft_key(subject: &str, node_id: &str) -> String {
    format!("{EDIT_DRAFT_PREFIX}{subject}:{node_id}")
}

/// Storage key of `subject`'s new-node (create-mode) draft. Keyed by user
/// like edit drafts, so the next account in the same browser never gets
/// someone else's half-written node restored on `/nodes/new`.
pub fn create_draft_key(subject: &str) -> String {
    format!("{CREATE_DRAFT_PREFIX}{subject}")
}

/// `localStorage` key prefix for create-mode drafts ([`create_draft_key`]).
pub const CREATE_DRAFT_PREFIX: &str = "ember_trove_create_draft:";

/// The pre-#161 create-draft key, shared by every account in the browser.
/// It is deleted, never migrated: nothing says whose draft it was.
pub const LEGACY_CREATE_DRAFT_KEY: &str = "ember_trove_create_draft";

/// Whether `key` is an editor draft that logout must wipe: any user's edit
/// or create draft, or the legacy unscoped create draft.
pub fn is_draft_key(key: &str) -> bool {
    key == LEGACY_CREATE_DRAFT_KEY
        || key.starts_with(CREATE_DRAFT_PREFIX)
        || key.starts_with(EDIT_DRAFT_PREFIX)
}

/// Which save a settled response belongs to (see [`SaveWatch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// The save the editor is waiting on: it drives the label and the lock.
    Current,
    /// A save the watchdog gave up on, or one a newer save replaced. It must
    /// not touch the label or the lock.
    Late,
}

/// Numbers the editor's saves so the watchdog and responses act only on
/// the save they belong to.
///
/// The give-up timer does not wait for the request to fail (#161 follow-up):
/// a save caught by the 401 refresh path in `api::handle_error_response`
/// never settles, so aborting it changed nothing and the editor sat on
/// "Still saving…" forever. Giving up now releases the lock by itself.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SaveWatch {
    last: u32,
    current: Option<u32>,
}

impl SaveWatch {
    /// Start a save; it becomes the current one.
    pub fn begin(&mut self) -> u32 {
        self.last = self.last.wrapping_add(1);
        self.current = Some(self.last);
        self.last
    }

    /// Whether a save is being waited on (Save and autosave hold off).
    pub fn in_flight(&self) -> bool {
        self.current.is_some()
    }

    /// Whether save `id` is still the one being waited on.
    pub fn is_current(&self, id: u32) -> bool {
        self.current == Some(id)
    }

    /// The give-up timer of save `id` fired. `true` when it was still
    /// current: the editor must show the failure and allow a new save now,
    /// whatever the request itself does later.
    pub fn give_up(&mut self, id: u32) -> bool {
        let was = self.is_current(id);
        if was {
            self.current = None;
        }
        was
    }

    /// A response for save `id` arrived.
    pub fn settle(&mut self, id: u32) -> Settled {
        if self.is_current(id) {
            self.current = None;
            Settled::Current
        } else {
            Settled::Late
        }
    }
}

/// Who changed the node when a save's precondition failed (HTTP 412).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// The server holds text this editor sent (an earlier save it gave up
    /// on landed late) or its unchanged baseline (a pin elsewhere moved the
    /// version): re-send the editor's text on top, no one's work is lost.
    OwnWrite,
    /// The text changed somewhere else. Don't overwrite it unasked.
    Foreign,
}

/// Classify a 412 from the server's current text fingerprint and the
/// fingerprints of the baseline and every snapshot this editor sent.
pub fn classify_conflict(server: u64, own: &[u64]) -> Conflict {
    if own.contains(&server) {
        Conflict::OwnWrite
    } else {
        Conflict::Foreign
    }
}

/// Stable 64-bit FNV-1a fingerprint of a (title, body, status) snapshot.
/// Not a security boundary, only an equality check that is cheap to keep
/// for every sent snapshot; stable across builds (unlike `DefaultHasher`).
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

/// Unsaved edits of an existing node, kept on this device until a save
/// lands. The fields match the editor's (title, body, status) snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditDraft {
    pub title: String,
    pub body: String,
    pub status: String,
}

impl EditDraft {
    /// Parse a stored draft. Corrupt payloads are treated as "no draft".
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }

    /// What to do with this draft when the node loads with `server` as its
    /// saved (title, body, status).
    pub fn recovery(&self, server: (&str, &str, &str)) -> DraftRecovery {
        if (
            self.title.as_str(),
            self.body.as_str(),
            self.status.as_str(),
        ) == server
        {
            // The save landed after all (#161 sighting 2): nothing to offer.
            DraftRecovery::Discard
        } else {
            DraftRecovery::Offer
        }
    }
}

/// The editor's decision for a stored draft, see [`EditDraft::recovery`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftRecovery {
    /// The server already has these edits; drop the draft silently.
    Discard,
    /// The draft differs from the server; offer Restore / Discard.
    Offer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_draft_keys_are_per_user_and_logout_wipes_every_draft_kind() {
        assert_eq!(create_draft_key("sub-a"), "ember_trove_create_draft:sub-a");
        assert_ne!(create_draft_key("sub-a"), create_draft_key("sub-b"));
        assert_ne!(create_draft_key("sub-a"), LEGACY_CREATE_DRAFT_KEY);
        assert!(is_draft_key(&create_draft_key("x")));
        assert!(is_draft_key(&edit_draft_key("x", "n")));
        assert!(is_draft_key(LEGACY_CREATE_DRAFT_KEY));
        assert!(!is_draft_key("ember_trove_theme"));
        assert!(!is_draft_key("ember_trove_create_draftish"));
    }

    #[test]
    fn a_save_that_never_settles_is_given_up_and_unlocks() {
        // The 401 refresh path parks the request forever: no response ever
        // arrives, so only the timer can release the lock.
        let mut w = SaveWatch::default();
        let a = w.begin();
        assert!(w.in_flight());
        assert!(w.give_up(a), "the timer flips the editor to failed");
        assert!(!w.in_flight(), "Save and autosave work again");
    }

    #[test]
    fn a_settled_save_ignores_its_own_timer() {
        let mut w = SaveWatch::default();
        let a = w.begin();
        assert_eq!(w.settle(a), Settled::Current);
        assert!(!w.give_up(a), "a late timer must not fail a finished save");
        assert!(!w.in_flight());
    }

    #[test]
    fn a_late_response_never_drives_the_retry() {
        let mut w = SaveWatch::default();
        let a = w.begin();
        assert!(w.give_up(a));
        let b = w.begin();
        // The aborted request rejects, or lands, after the retry started.
        assert_eq!(w.settle(a), Settled::Late);
        assert!(w.is_current(b), "the retry is still awaited");
        assert!(!w.give_up(a), "a's stale timer can't fail b");
        assert_eq!(w.settle(b), Settled::Current);
    }

    #[test]
    fn a_conflict_with_our_own_text_is_resent_a_foreign_one_is_not() {
        let base = fingerprint("T", "old", "draft");
        let sent = fingerprint("T", "old+", "draft");
        let own = [base, sent];
        assert_eq!(classify_conflict(sent, &own), Conflict::OwnWrite);
        assert_eq!(classify_conflict(base, &own), Conflict::OwnWrite);
        assert_eq!(
            classify_conflict(fingerprint("T", "elsewhere", "draft"), &own),
            Conflict::Foreign
        );
    }

    #[test]
    fn fingerprint_is_pinned_and_field_separated() {
        // Pinned: the value must not change between builds.
        assert_eq!(fingerprint("", "", ""), 0x0879_e907_b528_1763);
        assert_eq!(fingerprint("T", "old", "draft"), 0xf78e_de3b_361f_7ac1);
        assert_ne!(fingerprint("ab", "c", "d"), fingerprint("a", "bc", "d"));
    }

    fn draft(body: &str) -> EditDraft {
        EditDraft {
            title: "Plan".into(),
            body: body.into(),
            status: "draft".into(),
        }
    }

    #[test]
    fn the_stall_shows_before_the_save_is_given_up() {
        const { assert!(STALL_AFTER_MS < GIVE_UP_AFTER_MS) };
    }

    #[test]
    fn key_is_scoped_to_user_and_node() {
        let a = edit_draft_key("sub-a", "n1");
        assert_eq!(a, "ember_trove_edit_draft:sub-a:n1");
        assert_ne!(a, edit_draft_key("sub-b", "n1"));
        assert_ne!(a, edit_draft_key("sub-a", "n2"));
        assert!(a.starts_with(EDIT_DRAFT_PREFIX));
    }

    #[test]
    fn parse_round_trips_and_rejects_corrupt_json() {
        let d = draft("line one\n");
        let json = serde_json::to_string(&d).expect("serialize");
        assert_eq!(EditDraft::parse(&json), Some(d));
        assert_eq!(EditDraft::parse("{nope"), None);
        assert_eq!(EditDraft::parse(r#"{"title":"x"}"#), None);
    }

    #[test]
    fn a_draft_the_server_already_has_is_discarded() {
        let d = draft("text\n");
        assert_eq!(
            d.recovery(("Plan", "text\n", "draft")),
            DraftRecovery::Discard
        );
    }

    #[test]
    fn a_draft_that_differs_in_any_field_is_offered() {
        let d = draft("text\n");
        // The lost Enter keystroke of #161 sighting 1.
        assert_eq!(d.recovery(("Plan", "text", "draft")), DraftRecovery::Offer);
        assert_eq!(
            d.recovery(("Other", "text\n", "draft")),
            DraftRecovery::Offer
        );
        assert_eq!(
            d.recovery(("Plan", "text\n", "published")),
            DraftRecovery::Offer
        );
    }
}
