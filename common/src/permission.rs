use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, PermissionId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRole {
    Owner,
    Editor,
    Viewer,
}

wire_enum!(PermissionRole {
    Owner => "owner",
    Editor => "editor",
    Viewer => "viewer",
});

impl PermissionRole {
    /// Whether a holder of this role may do what `required` allows. The
    /// roles are ordered, weakest first: Viewer, Editor, Owner. The one
    /// rule for the API's per-node checks and for what the app offers.
    #[must_use]
    pub fn satisfies(&self, required: &PermissionRole) -> bool {
        match required {
            PermissionRole::Viewer => true,
            PermissionRole::Editor => {
                matches!(self, PermissionRole::Editor | PermissionRole::Owner)
            }
            PermissionRole::Owner => matches!(self, PermissionRole::Owner),
        }
    }
}

/// Whether `subject` holds an editor's grant on a node, among `grants`:
/// the rows `GET /nodes/{id}/permissions` listed to them (their own, or
/// every row for the node's owner). Only their own rows count. The app
/// uses it to offer a write control only to someone the API would let
/// write; the API checks again.
#[must_use]
pub fn holds_editor(subject: &str, grants: &[Permission]) -> bool {
    grants
        .iter()
        .any(|g| g.subject_id == subject && g.role.satisfies(&PermissionRole::Editor))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Permission {
    pub id: PermissionId,
    pub node_id: NodeId,
    pub subject_id: String,
    pub role: PermissionRole,
    pub granted_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct GrantPermissionRequest {
    #[garde(length(min = 1))]
    pub subject_id: String,
    #[garde(skip)]
    pub role: PermissionRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdatePermissionRequest {
    #[garde(skip)]
    pub role: PermissionRole,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct PermissionListParams {
    /// Filter by node when supplied.
    pub node_id: Option<uuid::Uuid>,
}

/// Request body for `POST /nodes/{id}/invite`.
///
/// If a Cognito user with the given email already exists, the permission is
/// granted directly.  Otherwise a new Cognito user is created (which triggers
/// a welcome / temporary-password email) and the permission is then granted.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct InviteRequest {
    #[garde(email)]
    pub email: String,
    #[garde(skip)]
    pub role: PermissionRole,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_permission_request_round_trip() {
        let req = UpdatePermissionRequest {
            role: PermissionRole::Editor,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: UpdatePermissionRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.role, req.role);
    }

    #[test]
    fn a_role_allows_what_the_weaker_roles_allow() {
        use PermissionRole::{Editor, Owner, Viewer};
        for (role, may) in [
            (Viewer, [true, false, false]),
            (Editor, [true, true, false]),
            (Owner, [true, true, true]),
        ] {
            for (required, expect) in [Viewer, Editor, Owner].iter().zip(may) {
                assert_eq!(
                    role.satisfies(required),
                    expect,
                    "{role:?} for {required:?}"
                );
            }
        }
    }

    #[test]
    fn only_ones_own_editor_or_owner_grant_lets_one_write() {
        let grant = |subject: &str, role: PermissionRole| Permission {
            id: PermissionId(uuid::Uuid::nil()),
            node_id: NodeId(uuid::Uuid::nil()),
            subject_id: subject.to_string(),
            role,
            granted_by: "someone".to_string(),
            created_at: chrono::DateTime::UNIX_EPOCH,
        };
        assert!(!holds_editor("me", &[]));
        assert!(!holds_editor("me", &[grant("me", PermissionRole::Viewer)]));
        assert!(holds_editor("me", &[grant("me", PermissionRole::Editor)]));
        assert!(holds_editor("me", &[grant("me", PermissionRole::Owner)]));
        // Someone else's grant on the node is not mine (an owner is
        // listed every row).
        assert!(!holds_editor(
            "me",
            &[
                grant("them", PermissionRole::Owner),
                grant("me", PermissionRole::Viewer)
            ]
        ));
        // Granting it gives no right.
        let mut by_me = grant("them", PermissionRole::Editor);
        by_me.granted_by = "me".to_string();
        assert!(!holds_editor("me", &[by_me]));
    }

    #[test]
    fn permission_role_serde_snake_case() {
        // PermissionRole must serialize as snake_case strings.
        assert_eq!(
            serde_json::to_string(&PermissionRole::Owner).unwrap(),
            "\"owner\""
        );
        assert_eq!(
            serde_json::to_string(&PermissionRole::Editor).unwrap(),
            "\"editor\""
        );
        assert_eq!(
            serde_json::to_string(&PermissionRole::Viewer).unwrap(),
            "\"viewer\""
        );
    }

    #[test]
    fn permission_role_deserialize_snake_case() {
        let owner: PermissionRole = serde_json::from_str("\"owner\"").unwrap();
        assert_eq!(owner, PermissionRole::Owner);
        let editor: PermissionRole = serde_json::from_str("\"editor\"").unwrap();
        assert_eq!(editor, PermissionRole::Editor);
        let viewer: PermissionRole = serde_json::from_str("\"viewer\"").unwrap();
        assert_eq!(viewer, PermissionRole::Viewer);
    }

    #[test]
    fn permission_list_params_defaults_to_no_filter() {
        let params = PermissionListParams::default();
        assert!(params.node_id.is_none());
    }

    #[test]
    fn permission_list_params_with_node_id() {
        let id = uuid::Uuid::new_v4();
        let json = format!("{{\"node_id\":\"{id}\"}}");
        let params: PermissionListParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params.node_id, Some(id));
    }

    #[test]
    fn grant_permission_request_validates_subject_id() {
        use garde::Validate;
        let valid = GrantPermissionRequest {
            subject_id: "user-123".to_string(),
            role: PermissionRole::Viewer,
        };
        assert!(valid.validate().is_ok());

        let invalid = GrantPermissionRequest {
            subject_id: String::new(), // length < 1
            role: PermissionRole::Viewer,
        };
        assert!(invalid.validate().is_err());
    }
}
