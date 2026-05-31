use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, NoteId};

fn default_note_color() -> String {
    "default".to_string()
}

/// A note. Attached to a node, or standalone (`node_id: None`) for an
/// inbox / micro-blog entry that isn't associated with any node.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Note {
    pub id: NoteId,
    /// Parent node, or `None` for a standalone (inbox) note.
    pub node_id: Option<NodeId>,
    pub owner_id: String,
    pub body: String,
    /// Named palette key for the card background colour (e.g. "amber", "rose").
    /// "default" means neutral / no colour.
    #[serde(default = "default_note_color")]
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Note enriched with the parent node's title — used in the central feed.
/// `node_title` is `None` for standalone (inbox) notes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FeedNote {
    #[serde(flatten)]
    pub note: Note,
    pub node_title: Option<String>,
}

/// Request body for creating a new note.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateNoteRequest {
    #[garde(length(min = 1, max = 10000))]
    pub body: String,
    /// Palette key: "default" | "amber" | "rose" | "lime" | "sky" | "violet".
    #[garde(length(min = 1, max = 20))]
    #[serde(default = "default_note_color")]
    pub color: String,
    /// Parent node. Absent / `None` creates a standalone (inbox) note.
    /// Only honored by the global `POST /notes` route; the node-scoped
    /// `POST /nodes/:id/notes` route always uses the node from the URL.
    #[garde(skip)]
    #[serde(default)]
    pub node_id: Option<NodeId>,
}

/// Sort order for the notes feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum NoteSort {
    /// Newest created first (default).
    #[default]
    Newest,
    /// Oldest created first.
    Oldest,
    /// Most recently updated first.
    Updated,
}

impl NoteSort {
    /// Parse the `sort` query value; anything unrecognized falls back to `Newest`.
    #[must_use]
    pub fn from_param(s: Option<&str>) -> Self {
        match s {
            Some("oldest") => Self::Oldest,
            Some("updated") => Self::Updated,
            _ => Self::Newest,
        }
    }
}

/// Query parameters for the notes feed (filter + sort); all optional.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct NoteFeedParams {
    /// Filter to a specific node (UUID string).
    pub node_id: Option<String>,
    /// When `true`, return only standalone (inbox) notes.
    pub uncategorized: Option<bool>,
    /// Inclusive lower bound on `created_at` (`YYYY-MM-DD`).
    pub from: Option<String>,
    /// Inclusive upper bound on `created_at` (`YYYY-MM-DD`).
    pub to: Option<String>,
    /// Case-insensitive substring filter on the note body.
    pub q: Option<String>,
    /// Sort order: `newest` (default) | `oldest` | `updated`.
    pub sort: Option<String>,
    /// 1-based page number (default 1).
    pub page: Option<u32>,
    /// Rows per page (default 200, server-clamped to 1000).
    pub per_page: Option<u32>,
}

/// Request body for editing an existing note.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateNoteRequest {
    #[garde(length(min = 1, max = 10000))]
    pub body: String,
    /// Palette key for the note background.
    #[garde(length(min = 1, max = 20))]
    #[serde(default = "default_note_color")]
    pub color: String,
}
