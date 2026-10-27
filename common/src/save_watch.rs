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
