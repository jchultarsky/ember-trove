//! Local drafts of modal forms (#170).
//!
//! A dialog's input used to live only in signals, so a reload (the update
//! banner invites one), a closed tab or an in-app navigation dropped it
//! without a word. The person editor now keeps what was typed on this
//! device, under the same rules as the node editor's drafts (#161): keyed
//! by user, wiped on logout ([`crate::save_watch::is_draft_key`]), stamped
//! and expired after [`crate::save_watch::DRAFT_TTL_MS`]. The keys and the
//! restore decision are pure, so they live here where they are tested.
//!
//! These drafts hold third parties' contact details. They never leave the
//! device, and the rules above bound how long they stay on it.

use serde::{Deserialize, Serialize};

use crate::id::{NodeId, PersonId};
use crate::person::PersonInput;

/// `localStorage` key prefix for modal-form drafts. Logout removes every
/// key with this prefix.
pub const FORM_DRAFT_PREFIX: &str = "ember_trove_form_draft:";

/// Which person dialog a draft belongs to. Each opening point has its own
/// draft, so a person half-typed on one node is never offered on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonForm {
    /// "New person" on the People page.
    New,
    /// "Create “…”" in a node's People section: the person plus the
    /// membership's Role and Note.
    NewOnNode(NodeId),
    /// "Edit person" on a person's card.
    Edit(PersonId),
}

impl PersonForm {
    /// Storage key of `subject`'s draft of this dialog.
    pub fn key(&self, subject: &str) -> String {
        match self {
            Self::New => format!("{FORM_DRAFT_PREFIX}{subject}:person:new"),
            Self::NewOnNode(n) => format!("{FORM_DRAFT_PREFIX}{subject}:person:new:node:{n}"),
            Self::Edit(p) => format!("{FORM_DRAFT_PREFIX}{subject}:person:{p}"),
        }
    }
}

/// What a person dialog held when the page went away.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersonFormDraft {
    pub person: PersonInput,
    /// The membership's Role and Note ([`PersonForm::NewOnNode`] only).
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub note: String,
    /// The values the dialog opened with. For an edit, that is the saved
    /// person, so a restore can say when the card changed since.
    #[serde(default)]
    pub base: PersonInput,
    /// When the draft was last written (ms since the epoch); the storage
    /// layer stamps it.
    #[serde(default)]
    pub saved_at: Option<i64>,
}

/// What the dialog does with a stored draft when it mounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormRecovery {
    /// Nothing in it that the dialog wouldn't start with: drop it silently.
    Discard,
    /// Reopen the dialog with the draft and say so. `changed` is true when
    /// the person was saved with other values after the draft was taken:
    /// saving the draft then replaces that change.
    Offer { changed: bool },
}

impl PersonFormDraft {
    /// Parse a stored draft. Corrupt payloads are treated as "no draft".
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }

    /// Whether the dialog holds anything beyond what it opened with. A
    /// clean dialog keeps no draft, so closing or reloading it changes
    /// nothing (#170, "keep the healthy paths").
    pub fn is_dirty(&self) -> bool {
        self.person != self.base || !self.role.trim().is_empty() || !self.note.trim().is_empty()
    }

    /// Decide the fate of this draft now that the dialog would open with
    /// `current` (the saved person for an edit, the prefill otherwise).
    pub fn recovery(&self, current: &PersonInput) -> FormRecovery {
        let membership_blank = self.role.trim().is_empty() && self.note.trim().is_empty();
        if self.person == *current && membership_blank {
            // The edit was saved after all, or there was never one.
            return FormRecovery::Discard;
        }
        FormRecovery::Offer {
            changed: self.base != *current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_watch::{Sweep, is_draft_key, sweep};

    fn named(n: &str) -> PersonInput {
        PersonInput {
            display_name: n.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn keys_are_per_user_and_per_dialog_and_logout_wipes_them() {
        let node = NodeId::new();
        let person = PersonId::new();
        let keys = [
            PersonForm::New.key("sub-a"),
            PersonForm::NewOnNode(node).key("sub-a"),
            PersonForm::Edit(person).key("sub-a"),
            PersonForm::New.key("sub-b"),
        ];
        for (i, a) in keys.iter().enumerate() {
            assert!(is_draft_key(a), "logout must wipe {a}");
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_ne!(
            PersonForm::NewOnNode(node).key("s"),
            PersonForm::NewOnNode(NodeId::new()).key("s"),
            "a draft on one node is never offered on another"
        );
    }

    #[test]
    fn a_clean_dialog_is_not_dirty() {
        let base = named("Ada");
        let d = PersonFormDraft {
            person: base.clone(),
            base,
            ..Default::default()
        };
        assert!(!d.is_dirty());
    }

    #[test]
    fn typed_fields_or_membership_make_it_dirty() {
        let d = PersonFormDraft {
            person: named("Ada"),
            ..Default::default()
        };
        assert!(d.is_dirty());
        let d = PersonFormDraft {
            role: "Sponsor".into(),
            ..Default::default()
        };
        assert!(d.is_dirty());
        let d = PersonFormDraft {
            note: "  ".into(),
            ..Default::default()
        };
        assert!(!d.is_dirty(), "whitespace alone is nothing to keep");
    }

    #[test]
    fn a_draft_matching_the_saved_person_is_discarded() {
        let d = PersonFormDraft {
            person: named("Ada Lovelace"),
            base: named("Ada"),
            ..Default::default()
        };
        assert_eq!(d.recovery(&named("Ada Lovelace")), FormRecovery::Discard);
    }

    #[test]
    fn an_edit_on_an_unchanged_person_is_offered_plainly() {
        let d = PersonFormDraft {
            person: named("Ada Lovelace"),
            base: named("Ada"),
            ..Default::default()
        };
        assert_eq!(
            d.recovery(&named("Ada")),
            FormRecovery::Offer { changed: false }
        );
    }

    #[test]
    fn an_edit_on_a_person_changed_since_says_so() {
        let d = PersonFormDraft {
            person: named("Ada Lovelace"),
            base: named("Ada"),
            ..Default::default()
        };
        assert_eq!(
            d.recovery(&named("Ada King")),
            FormRecovery::Offer { changed: true }
        );
    }

    #[test]
    fn membership_alone_is_offered() {
        let d = PersonFormDraft {
            role: "Sponsor".into(),
            ..Default::default()
        };
        assert_eq!(
            d.recovery(&PersonInput::default()),
            FormRecovery::Offer { changed: false }
        );
    }

    #[test]
    fn parse_round_trips_and_rejects_garbage() {
        let d = PersonFormDraft {
            person: named("Ada"),
            role: "Sponsor".into(),
            note: "met at the fair".into(),
            base: PersonInput::default(),
            saved_at: Some(1_800_000_000_000),
        };
        let json = serde_json::to_string(&d).expect("serialize");
        assert_eq!(PersonFormDraft::parse(&json), Some(d));
        assert_eq!(PersonFormDraft::parse("{not json"), None);
    }

    #[test]
    fn the_startup_sweep_expires_form_drafts_like_editor_drafts() {
        let old = PersonFormDraft {
            person: named("Ada"),
            saved_at: Some(0),
            ..Default::default()
        };
        let json = serde_json::to_string(&old).expect("serialize");
        assert_eq!(
            sweep(&json, crate::save_watch::DRAFT_TTL_MS + 1),
            Sweep::Expire
        );
    }
}
