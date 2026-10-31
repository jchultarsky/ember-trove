//! Captures (ONTOLOGY §9.1, #234): transient, unclassified input held in
//! the Inbox until it is processed into exactly one destination or
//! deliberately deleted.
//!
//! A capture is owner-only, with no admin bypass (owner decision,
//! 2026-10-31): it is personal input that hasn't become anything yet. It
//! is in backups, and in no one else's lists, search, export or webhooks.

use chrono::{DateTime, NaiveDate, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::backup::BackupNote;
use crate::id::CaptureId;

/// The longest a capture may be, in characters (the database's CHECK).
pub const MAX_LEN: usize = 10_000;

/// One capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Capture {
    pub id: CaptureId,
    pub owner_id: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `POST /captures` and `PATCH /captures/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CaptureRequest {
    #[garde(length(min = 1, max = MAX_LEN))]
    pub body: String,
}

/// A note with no node becomes a capture (#234): migration 043's rule, kept
/// here for restoring archives made before it (format 3). Same id, owner,
/// text and times. `None` for a note that has a node.
#[must_use]
pub fn from_nodeless_note(note: &BackupNote) -> Option<Capture> {
    note.node_id.is_none().then(|| Capture {
        id: CaptureId(note.id.0),
        owner_id: note.owner_id.clone(),
        body: note.body.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
    })
}

/// The calendar's capture for a day (#234): a capture has no date, so the
/// date leads its text and nothing the user picked is lost ("Fri 7 Nov:
/// call the bank").
#[must_use]
pub fn dated(date: NaiveDate, text: &str) -> String {
    format!("{}: {}", date.format("%a %-d %b"), text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{NodeId, NoteId};
    use chrono::TimeZone;

    fn note(node: Option<u128>) -> BackupNote {
        let at = Utc
            .with_ymd_and_hms(2026, 10, 1, 9, 0, 0)
            .single()
            .expect("date");
        BackupNote {
            id: NoteId(uuid::Uuid::from_u128(7)),
            node_id: node.map(|n| NodeId(uuid::Uuid::from_u128(n))),
            owner_id: "alice".into(),
            body: "an idea".into(),
            color: "default".into(),
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn only_a_nodeless_note_becomes_a_capture_keeping_its_id_and_times() {
        let c = from_nodeless_note(&note(None)).expect("a capture");
        assert_eq!(c.id.0, uuid::Uuid::from_u128(7));
        assert_eq!((c.owner_id.as_str(), c.body.as_str()), ("alice", "an idea"));
        assert_eq!(c.created_at, note(None).created_at);
        assert!(from_nodeless_note(&note(Some(1))).is_none());
    }

    #[test]
    fn a_dated_capture_leads_with_its_day() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 7).expect("date");
        assert_eq!(dated(day, "  call the bank "), "Sat 7 Nov: call the bank");
    }
}
