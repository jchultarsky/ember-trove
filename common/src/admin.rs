use garde::Validate;
use serde::{Deserialize, Serialize};

/// A Cognito user as returned by the Admin API, enriched with group memberships.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub enabled: bool,
    pub realm_roles: Vec<String>,
    /// Suspended in Ember Trove (#239): the API refuses the account's
    /// sessions and Cognito blocks its sign-in. Filled from the API's own
    /// list, not from Cognito.
    #[serde(default)]
    pub suspended: bool,
}

impl AdminUser {
    /// Display name: "First Last", falling back to username.
    pub fn display_name(&self) -> String {
        match (&self.first_name, &self.last_name) {
            (Some(f), Some(l)) if !f.is_empty() || !l.is_empty() => {
                format!("{} {}", f, l).trim().to_string()
            }
            (Some(f), _) if !f.is_empty() => f.clone(),
            _ => self.username.clone(),
        }
    }
}

/// Request body for creating a new Cognito user.
/// The email address is used as the Cognito username.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateAdminUserRequest {
    #[garde(length(min = 1, max = 254))]
    pub email: String,
    #[garde(length(max = 64))]
    pub first_name: String,
    #[garde(length(max = 64))]
    pub last_name: String,
    /// Realm roles to assign immediately after creation.
    #[garde(skip)]
    pub initial_roles: Vec<String>,
    /// If true, Cognito sends a temporary-password welcome email to the user.
    #[garde(skip)]
    pub send_welcome_email: bool,
}

/// What stands in for a deleted account's subject on what stays behind on
/// other people's nodes (#239): History entries, version authorship and
/// `granted_by`. A Cognito `sub` is a UUID, so this can't collide with one.
pub const DELETED_ACCOUNT: &str = "deleted-account";

/// The name shown in place of a deleted account's name or email.
pub const DELETED_ACCOUNT_NAME: &str = "Deleted account";

/// What deleting an account removes, or would remove (#239). Counts only:
/// the admin never sees a title or any content (ONTOLOGY §14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeCounts {
    /// Their nodes, with everything on them.
    pub nodes: u64,
    /// Of those, nodes shared with someone else, who loses access.
    pub shared_nodes: u64,
    /// Their notes, on any node.
    pub notes: u64,
    /// Their tasks that are deleted.
    pub tasks: u64,
    /// Their tasks on other people's nodes, which pass to the node's owner.
    pub tasks_handed_over: u64,
    pub captures: u64,
    pub people: u64,
    /// Files attached to their nodes.
    pub files: u64,
    /// Tags, templates, saved searches, favorites, webhooks and editor
    /// settings.
    pub settings: u64,
}

impl PurgeCounts {
    /// Nothing was (or would be) removed: a retry of a finished purge.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Request body for replacing a user's full set of realm roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRolesRequest {
    pub roles: Vec<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_user(first: Option<&str>, last: Option<&str>, username: &str) -> AdminUser {
        AdminUser {
            id: "id".to_string(),
            username: username.to_string(),
            email: None,
            first_name: first.map(str::to_string),
            last_name: last.map(str::to_string),
            enabled: true,
            realm_roles: vec![],
            suspended: false,
        }
    }

    #[test]
    fn display_name_full_name() {
        let u = make_user(Some("Jane"), Some("Doe"), "jdoe");
        assert_eq!(u.display_name(), "Jane Doe");
    }

    #[test]
    fn display_name_first_only() {
        let u = make_user(Some("Jane"), None, "jdoe");
        assert_eq!(u.display_name(), "Jane");
    }

    #[test]
    fn display_name_first_only_last_empty() {
        // last_name present but empty — first_name branch takes over.
        let u = make_user(Some("Jane"), Some(""), "jdoe");
        assert_eq!(u.display_name(), "Jane");
    }

    #[test]
    fn display_name_falls_back_to_username_when_both_empty() {
        let u = make_user(Some(""), Some(""), "jdoe");
        assert_eq!(u.display_name(), "jdoe");
    }

    #[test]
    fn display_name_falls_back_to_username_when_both_none() {
        let u = make_user(None, None, "jdoe");
        assert_eq!(u.display_name(), "jdoe");
    }

    #[test]
    fn display_name_trims_trailing_space_when_last_empty() {
        // first="Alice", last="" → format!("Alice ").trim() == "Alice"
        let u = make_user(Some("Alice"), Some(""), "alice");
        assert_eq!(u.display_name(), "Alice");
    }

    #[test]
    fn create_request_validation_rejects_empty_email() {
        use garde::Validate;
        let req = CreateAdminUserRequest {
            email: "".to_string(),
            first_name: "Jane".to_string(),
            last_name: "Doe".to_string(),
            initial_roles: vec![],
            send_welcome_email: false,
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn create_request_validation_accepts_valid_email() {
        use garde::Validate;
        let req = CreateAdminUserRequest {
            email: "jane@example.com".to_string(),
            first_name: "Jane".to_string(),
            last_name: "Doe".to_string(),
            initial_roles: vec!["user".to_string()],
            send_welcome_email: true,
        };
        assert!(req.validate().is_ok());
    }
}
