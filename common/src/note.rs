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
