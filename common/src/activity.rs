use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{ActivityId, NodeId};

/// A single entry in a node's activity log.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActivityEntry {
    pub id: ActivityId,
    pub node_id: NodeId,
    /// Cognito sub of the acting user.
    pub subject_id: String,
    /// Human-readable action verb.
    pub action: ActivityAction,
    /// Optional context — actor name/email, role, tag name, etc.
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Activity entry enriched with the parent node's title — used by the
/// v2.9.0 dashboard recap section so the UI doesn't have to fetch the
/// node separately for every line.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RecentActivityEntry {
    #[serde(flatten)]
    pub entry: ActivityEntry,
    pub node_title: String,
}

/// Enumeration of actions recorded in the activity log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityAction {
    Created,
    Edited,
    Deleted,
    TagAdded,
    TagRemoved,
    AttachmentUploaded,
    PermissionGranted,
    PermissionRevoked,
    Shared,
    Exported,
    CreatedFromTemplate,
}

impl ActivityAction {
    /// Stable string used when writing to / reading from the database.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Edited => "edited",
            Self::Deleted => "deleted",
            Self::TagAdded => "tag_added",
            Self::TagRemoved => "tag_removed",
            Self::AttachmentUploaded => "attachment_uploaded",
            Self::PermissionGranted => "permission_granted",
            Self::PermissionRevoked => "permission_revoked",
            Self::Shared => "shared",
            Self::Exported => "exported",
            Self::CreatedFromTemplate => "created_from_template",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "created" => Some(Self::Created),
            "edited" => Some(Self::Edited),
            "deleted" => Some(Self::Deleted),
            "tag_added" => Some(Self::TagAdded),
            "tag_removed" => Some(Self::TagRemoved),
            "attachment_uploaded" => Some(Self::AttachmentUploaded),
            "permission_granted" => Some(Self::PermissionGranted),
            "permission_revoked" => Some(Self::PermissionRevoked),
            "shared" => Some(Self::Shared),
            "exported" => Some(Self::Exported),
            "created_from_template" => Some(Self::CreatedFromTemplate),
            _ => None,
        }
    }

    /// Icon name from Material Symbols.
    #[must_use]
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Created => "add_circle",
            Self::Edited => "edit",
            Self::Deleted => "delete",
            Self::TagAdded => "label",
            Self::TagRemoved => "label_off",
            Self::AttachmentUploaded => "attach_file",
            Self::PermissionGranted => "person_add",
            Self::PermissionRevoked => "person_remove",
            Self::Shared => "link",
            Self::Exported => "download",
            Self::CreatedFromTemplate => "content_copy",
        }
    }

    /// Short human-readable past-tense label.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Edited => "edited",
            Self::Deleted => "deleted",
            Self::TagAdded => "added tag",
            Self::TagRemoved => "removed tag",
            Self::AttachmentUploaded => "uploaded attachment",
            Self::PermissionGranted => "granted access",
            Self::PermissionRevoked => "revoked access",
            Self::Shared => "created share link",
            Self::Exported => "exported",
            Self::CreatedFromTemplate => "created from template",
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_actions() {
        let actions = [
            ActivityAction::Created,
            ActivityAction::Edited,
            ActivityAction::Deleted,
            ActivityAction::TagAdded,
            ActivityAction::TagRemoved,
            ActivityAction::AttachmentUploaded,
            ActivityAction::PermissionGranted,
            ActivityAction::PermissionRevoked,
            ActivityAction::Shared,
            ActivityAction::Exported,
            ActivityAction::CreatedFromTemplate,
        ];
        for action in &actions {
            let s = action.as_str();
            let back = ActivityAction::from_db_str(s).expect("round-trip must succeed");
            assert_eq!(&back, action, "action {s} did not round-trip");
        }
    }

    #[test]
    fn from_str_unknown_returns_none() {
        assert!(ActivityAction::from_db_str("viewed").is_none());
        assert!(ActivityAction::from_db_str("").is_none());
    }
}
