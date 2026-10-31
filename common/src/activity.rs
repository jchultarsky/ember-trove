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

/// An `activity_log` row as stored in a backup archive.
///
/// `action` is the raw database string, not [`ActivityAction`]: a backup
/// may hold an action that the restoring build doesn't know (a newer
/// release wrote it), and that must not make the whole archive unreadable.
/// Unknown actions restore as they are and are skipped when read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityRecord {
    pub id: ActivityId,
    pub node_id: NodeId,
    pub subject_id: String,
    pub action: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Enumeration of actions recorded in the activity log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityAction {
    Created,
    Edited,
    // `Deleted` was here. It was written after the node was deleted, so the
    // FK on the per-node log always rejected it and no row ever existed
    // (#208); its database string "deleted" stays unused.
    TagAdded,
    TagRemoved,
    AttachmentUploaded,
    PermissionGranted,
    PermissionRevoked,
    Shared,
    Exported,
    CreatedFromTemplate,
    /// A node's project status changed (#208): History, ONTOLOGY §10.
    /// Metadata carries `from` and `to` (labels, either may be null).
    StatusChanged,
}

impl ActivityAction {
    /// Every action this build knows, in declaration order.
    pub const ALL: [Self; 11] = [
        Self::Created,
        Self::Edited,
        Self::TagAdded,
        Self::TagRemoved,
        Self::AttachmentUploaded,
        Self::PermissionGranted,
        Self::PermissionRevoked,
        Self::Shared,
        Self::Exported,
        Self::CreatedFromTemplate,
        Self::StatusChanged,
    ];

    /// The database strings of [`Self::ALL`]. Reads filter on these in SQL
    /// (#208), so a row from a newer release neither fails a list nor uses
    /// up its `LIMIT`.
    #[must_use]
    pub fn known_db_strs() -> Vec<&'static str> {
        Self::ALL.iter().map(Self::as_str).collect()
    }

    /// Stable string used when writing to / reading from the database.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Edited => "edited",
            Self::TagAdded => "tag_added",
            Self::TagRemoved => "tag_removed",
            Self::AttachmentUploaded => "attachment_uploaded",
            Self::PermissionGranted => "permission_granted",
            Self::PermissionRevoked => "permission_revoked",
            Self::Shared => "shared",
            Self::Exported => "exported",
            Self::CreatedFromTemplate => "created_from_template",
            Self::StatusChanged => "status_changed",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "created" => Some(Self::Created),
            "edited" => Some(Self::Edited),
            "tag_added" => Some(Self::TagAdded),
            "tag_removed" => Some(Self::TagRemoved),
            "attachment_uploaded" => Some(Self::AttachmentUploaded),
            "permission_granted" => Some(Self::PermissionGranted),
            "permission_revoked" => Some(Self::PermissionRevoked),
            "shared" => Some(Self::Shared),
            "exported" => Some(Self::Exported),
            "created_from_template" => Some(Self::CreatedFromTemplate),
            "status_changed" => Some(Self::StatusChanged),
            _ => None,
        }
    }

    /// Icon name from Material Symbols.
    #[must_use]
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Created => "add_circle",
            Self::Edited => "edit",
            Self::TagAdded => "label",
            Self::TagRemoved => "label_off",
            Self::AttachmentUploaded => "attach_file",
            Self::PermissionGranted => "person_add",
            Self::PermissionRevoked => "person_remove",
            Self::Shared => "link",
            Self::Exported => "download",
            Self::CreatedFromTemplate => "content_copy",
            Self::StatusChanged => "flag",
        }
    }

    /// Short human-readable past-tense label.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Edited => "edited",
            Self::TagAdded => "added tag",
            Self::TagRemoved => "removed tag",
            Self::AttachmentUploaded => "uploaded attachment",
            Self::PermissionGranted => "granted access",
            Self::PermissionRevoked => "revoked access",
            Self::Shared => "created share link",
            Self::Exported => "exported",
            Self::CreatedFromTemplate => "created from template",
            // Lowercase like the rest: the dashboard recap prints it inline.
            Self::StatusChanged => "changed status",
        }
    }
}

/// One line describing a project-status change, from a `status_changed`
/// entry's `from` / `to` labels (#208).
#[must_use]
pub fn status_change_summary(from: Option<&str>, to: Option<&str>) -> String {
    match (from, to) {
        (Some(a), Some(b)) => format!("{a} \u{2192} {b}"),
        (None, Some(b)) => format!("set to {b}"),
        (Some(a), None) => format!("{a} cleared"),
        (None, None) => "cleared".to_string(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_actions() {
        for action in &ActivityAction::ALL {
            let s = action.as_str();
            let back = ActivityAction::from_db_str(s).expect("round-trip must succeed");
            assert_eq!(&back, action, "action {s} did not round-trip");
        }
        assert_eq!(
            ActivityAction::known_db_strs().len(),
            ActivityAction::ALL.len()
        );
    }

    #[test]
    fn all_lists_every_variant_once() {
        // Exhaustive on purpose: a new variant fails to compile here until
        // it's given an ordinal and added to `ALL`.
        fn ordinal(a: &ActivityAction) -> usize {
            match a {
                ActivityAction::Created => 0,
                ActivityAction::Edited => 1,
                ActivityAction::TagAdded => 2,
                ActivityAction::TagRemoved => 3,
                ActivityAction::AttachmentUploaded => 4,
                ActivityAction::PermissionGranted => 5,
                ActivityAction::PermissionRevoked => 6,
                ActivityAction::Shared => 7,
                ActivityAction::Exported => 8,
                ActivityAction::CreatedFromTemplate => 9,
                ActivityAction::StatusChanged => 10,
            }
        }
        for (i, a) in ActivityAction::ALL.iter().enumerate() {
            assert_eq!(ordinal(a), i, "{} out of place in ALL", a.as_str());
        }
    }

    #[test]
    fn the_never_stored_deleted_action_is_no_longer_known() {
        // #208: it could never be written, so it's not in the vocabulary.
        assert!(ActivityAction::from_db_str("deleted").is_none());
        assert!(!ActivityAction::known_db_strs().contains(&"deleted"));
    }

    #[test]
    fn from_str_unknown_returns_none() {
        assert!(ActivityAction::from_db_str("viewed").is_none());
        assert!(ActivityAction::from_db_str("").is_none());
    }

    #[test]
    fn a_status_change_reads_as_from_arrow_to() {
        assert_eq!(
            status_change_summary(Some("Active"), Some("Blocked")),
            "Active \u{2192} Blocked"
        );
        assert_eq!(status_change_summary(None, Some("Active")), "set to Active");
        assert_eq!(
            status_change_summary(Some("Active"), None),
            "Active cleared"
        );
        assert_eq!(status_change_summary(None, None), "cleared");
    }

    #[test]
    fn status_changed_is_stored_as_status_changed() {
        assert_eq!(ActivityAction::StatusChanged.as_str(), "status_changed");
        assert_eq!(ActivityAction::StatusChanged.label(), "changed status");
    }
}
