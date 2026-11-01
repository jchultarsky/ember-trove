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
use crate::id::{CaptureId, NodeId, NoteId, TaskId};
use crate::node::NodeType;
use crate::task::TaskPriority;

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

/// `POST /captures` and `PATCH /captures/{id}`. The limit counts
/// characters, like the database's `char_length` CHECK; garde's plain
/// `length` counts bytes, which refused long non-ASCII captures.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CaptureRequest {
    #[garde(length(chars, min = 1, max = MAX_LEN))]
    pub body: String,
}

/// The longest title a node or a Task may have, in characters (the
/// database's CHECKs).
pub const TITLE_MAX_LEN: usize = 500;

/// `POST /captures/{id}/convert` (#234, ONTOLOGY §9.1): the one thing a
/// capture becomes. Its text is copied or reworded by the user; the
/// capture is then deleted, with no link back to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "into", rename_all = "snake_case")]
pub enum ConvertCaptureRequest {
    /// A new node of any kind, optionally with its home (a `contains`
    /// link from `home`).
    Node {
        node_type: NodeType,
        title: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        home: Option<NodeId>,
    },
    /// A note on an existing node (§9.2).
    Note { node_id: NodeId, body: String },
    /// A Task in a Project (§9.3: a Task needs its Project).
    Task {
        node_id: NodeId,
        title: String,
        #[serde(default)]
        due_date: Option<NaiveDate>,
        #[serde(default)]
        priority: Option<TaskPriority>,
    },
}

impl ConvertCaptureRequest {
    /// The request with its text trimmed, or why it can't be saved. Limits
    /// count characters, as the database's CHECKs do. An empty node body
    /// becomes `None`.
    ///
    /// # Errors
    /// The 422 message: a blank or over-long title or note.
    pub fn checked(self) -> Result<Self, String> {
        fn text(s: &str, what: &str, max: usize) -> Result<String, String> {
            let t = s.trim();
            if t.is_empty() {
                return Err(format!("{what} can't be empty"));
            }
            if t.chars().count() > max {
                return Err(format!("{what} is longer than {max} characters"));
            }
            Ok(t.to_string())
        }
        Ok(match self {
            Self::Node {
                node_type,
                title,
                body,
                home,
            } => Self::Node {
                node_type,
                title: text(&title, "the title", TITLE_MAX_LEN)?,
                body: body.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()),
                home,
            },
            Self::Note { node_id, body } => Self::Note {
                node_id,
                body: text(&body, "the note", MAX_LEN)?,
            },
            Self::Task {
                node_id,
                title,
                due_date,
                priority,
            } => Self::Task {
                node_id,
                title: text(&title, "the title", TITLE_MAX_LEN)?,
                due_date,
                priority,
            },
        })
    }
}

/// What a capture became: enough for the Inbox's toast to name it and
/// open it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "into", rename_all = "snake_case")]
pub enum Converted {
    Node { id: NodeId },
    Note { id: NoteId, node_id: NodeId },
    Task { id: TaskId, node_id: NodeId },
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
    fn the_limit_counts_characters_like_the_database() {
        // Two bytes a character: 10 000 of them is 20 000 bytes, still one
        // capture's worth (the CHECK is `char_length`).
        let full = CaptureRequest {
            body: "é".repeat(MAX_LEN),
        };
        assert!(full.validate().is_ok());
        let over = CaptureRequest {
            body: "é".repeat(MAX_LEN + 1),
        };
        assert!(over.validate().is_err());
    }

    #[test]
    fn a_convert_request_names_its_destination() {
        let req: ConvertCaptureRequest = serde_json::from_value(serde_json::json!({
            "into": "task",
            "node_id": uuid::Uuid::from_u128(3),
            "title": "call the bank",
        }))
        .expect("a task");
        assert!(matches!(
            req,
            ConvertCaptureRequest::Task {
                due_date: None,
                priority: None,
                ..
            }
        ));
        // A Task without its Project is not a request at all (§9.3).
        assert!(
            serde_json::from_value::<ConvertCaptureRequest>(
                serde_json::json!({ "into": "task", "title": "x" })
            )
            .is_err()
        );
        let out = serde_json::to_value(Converted::Node {
            id: NodeId(uuid::Uuid::from_u128(4)),
        })
        .expect("json");
        assert_eq!(out["into"], "node");
    }

    #[test]
    fn a_convert_request_is_trimmed_and_limited_in_characters() {
        let node = |title: String, body: Option<&str>| ConvertCaptureRequest::Node {
            node_type: NodeType::Article,
            title,
            body: body.map(str::to_string),
            home: None,
        };
        // Two bytes a character: 500 of them fit a title.
        assert!(node("é".repeat(TITLE_MAX_LEN), None).checked().is_ok());
        assert!(node("é".repeat(TITLE_MAX_LEN + 1), None).checked().is_err());
        assert!(node("   ".into(), None).checked().is_err());
        assert_eq!(
            node(" Idea ".into(), Some("  ")).checked(),
            Ok(node("Idea".into(), None))
        );

        let note = |body: String| ConvertCaptureRequest::Note {
            node_id: NodeId(uuid::Uuid::from_u128(1)),
            body,
        };
        assert!(note("ж".repeat(MAX_LEN)).checked().is_ok());
        assert!(note("ж".repeat(MAX_LEN + 1)).checked().is_err());
        assert!(note("\n ".into()).checked().is_err());

        let task = ConvertCaptureRequest::Task {
            node_id: NodeId(uuid::Uuid::from_u128(1)),
            title: "x".repeat(TITLE_MAX_LEN + 1),
            due_date: None,
            priority: None,
        };
        assert!(
            task.checked().is_err(),
            "a Task's title must fit (owner decision)"
        );
    }

    #[test]
    fn a_dated_capture_leads_with_its_day() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 7).expect("date");
        assert_eq!(dated(day, "  call the bank "), "Sat 7 Nov: call the bank");
    }
}
