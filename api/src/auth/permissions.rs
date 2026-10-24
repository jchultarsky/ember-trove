/// Per-node permission check helpers.
///
/// Each helper performs a DB lookup and returns `Err(ApiError::Forbidden)`
/// when the caller does not hold the required minimum role on the node.
///
/// Role hierarchy (weakest → strongest):  Viewer < Editor < Owner
use common::auth::AuthClaims;
use common::id::NodeId;
use common::permission::PermissionRole;

use crate::error::ApiError;
use crate::repo::permission::PermissionRepo;

/// Returns `true` when `actual` satisfies or exceeds `required`.
fn role_satisfies(actual: &PermissionRole, required: &PermissionRole) -> bool {
    match required {
        PermissionRole::Viewer => true, // every role can view
        PermissionRole::Editor => {
            matches!(actual, PermissionRole::Editor | PermissionRole::Owner)
        }
        PermissionRole::Owner => matches!(actual, PermissionRole::Owner),
    }
}

/// Returns `true` when the caller holds the `admin` role in their OIDC groups.
///
/// Admins bypass per-node permission checks and can read/write all nodes.
pub fn is_admin(claims: &AuthClaims) -> bool {
    claims.roles.contains(&"admin".to_string())
}

/// The owner scope for listing/visibility queries: `None` for an admin (every
/// owner), otherwise the caller's subject. Repo methods treat `None` as
/// "no owner filter".
pub fn subject_scope(claims: &AuthClaims) -> Option<&str> {
    if is_admin(claims) {
        None
    } else {
        Some(claims.sub.as_str())
    }
}

/// SECURITY: protect the canonical creator's (`nodes.owner_id`) Owner row.
///
/// `require_role` consults only the permissions table, so that row is the
/// backstop that keeps a creator from being locked out of their own node. A
/// granted (non-canonical) Owner must not revoke it (`new_role = None`) or
/// downgrade it (`Some(role)` weaker than Owner); only an admin may. Every
/// grant/update/revoke path calls this — the standalone `/permissions/{id}`
/// routes used to skip it.
pub fn guard_canonical_owner(
    claims: &AuthClaims,
    node_owner_id: &str,
    subject_id: &str,
    new_role: Option<&PermissionRole>,
) -> Result<(), ApiError> {
    let weakens = !matches!(new_role, Some(PermissionRole::Owner));
    if subject_id == node_owner_id && weakens && !is_admin(claims) {
        return Err(ApiError::Forbidden(
            "cannot revoke or downgrade the node owner's permission".to_string(),
        ));
    }
    Ok(())
}

/// Assert that the caller holds the `admin` role.
///
/// Returns `Err(ApiError::Forbidden)` when the caller is not an admin.
pub fn require_admin(claims: &AuthClaims) -> Result<(), ApiError> {
    if is_admin(claims) {
        Ok(())
    } else {
        Err(ApiError::Forbidden("admin role required".to_string()))
    }
}

/// Assert that the caller owns the resource (by comparing subject IDs) or is
/// an admin.
///
/// Use this for resources that have a direct owner field (tags, templates,
/// tasks, backups) rather than node-level permission rows.
pub fn require_resource_owner(claims: &AuthClaims, owner_id: &str) -> Result<(), ApiError> {
    if claims.sub == owner_id || is_admin(claims) {
        Ok(())
    } else {
        Err(ApiError::Forbidden("access denied".to_string()))
    }
}

/// Assert that `claims.sub` holds at least `required_role` on `node_id`.
///
/// Users in the `admin` OIDC group bypass the per-node check entirely.
/// Returns `Err(ApiError::Forbidden)` when no permission row is found or the
/// stored role is insufficient.
pub async fn require_role(
    permissions: &dyn PermissionRepo,
    claims: &AuthClaims,
    node_id: NodeId,
    required_role: PermissionRole,
) -> Result<(), ApiError> {
    if is_admin(claims) {
        return Ok(());
    }

    let perm = permissions.find(node_id, &claims.sub).await?;

    match perm {
        None => Err(ApiError::Forbidden("access denied".to_string())),
        Some(p) if role_satisfies(&p.role, &required_role) => Ok(()),
        Some(_) => Err(ApiError::Forbidden("insufficient permission".to_string())),
    }
}

pub async fn require_owner(
    permissions: &dyn PermissionRepo,
    claims: &AuthClaims,
    node_id: NodeId,
) -> Result<(), ApiError> {
    require_role(permissions, claims, node_id, PermissionRole::Owner).await
}

pub async fn require_editor(
    permissions: &dyn PermissionRepo,
    claims: &AuthClaims,
    node_id: NodeId,
) -> Result<(), ApiError> {
    require_role(permissions, claims, node_id, PermissionRole::Editor).await
}

pub async fn require_viewer(
    permissions: &dyn PermissionRepo,
    claims: &AuthClaims,
    node_id: NodeId,
) -> Result<(), ApiError> {
    require_role(permissions, claims, node_id, PermissionRole::Viewer).await
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_requirement_is_satisfied_by_any_role() {
        for role in [
            PermissionRole::Viewer,
            PermissionRole::Editor,
            PermissionRole::Owner,
        ] {
            assert!(role_satisfies(&role, &PermissionRole::Viewer));
        }
    }

    #[test]
    fn editor_requirement_is_satisfied_by_editor_and_owner() {
        assert!(role_satisfies(
            &PermissionRole::Editor,
            &PermissionRole::Editor
        ));
        assert!(role_satisfies(
            &PermissionRole::Owner,
            &PermissionRole::Editor
        ));
        assert!(!role_satisfies(
            &PermissionRole::Viewer,
            &PermissionRole::Editor
        ));
    }

    #[test]
    fn owner_requirement_is_satisfied_only_by_owner() {
        assert!(role_satisfies(
            &PermissionRole::Owner,
            &PermissionRole::Owner
        ));
        assert!(!role_satisfies(
            &PermissionRole::Editor,
            &PermissionRole::Owner
        ));
        assert!(!role_satisfies(
            &PermissionRole::Viewer,
            &PermissionRole::Owner
        ));
    }

    #[test]
    fn admin_role_bypasses_permission_check() {
        let admin_claims = AuthClaims {
            sub: "admin-sub".to_string(),
            email: None,
            name: None,
            roles: vec!["admin".to_string()],
            exp: i64::MAX,
        };
        assert!(is_admin(&admin_claims));

        let non_admin = AuthClaims {
            sub: "user-sub".to_string(),
            email: None,
            name: None,
            roles: vec!["viewer".to_string()],
            exp: i64::MAX,
        };
        assert!(!is_admin(&non_admin));

        let no_roles = AuthClaims {
            sub: "user-sub".to_string(),
            email: None,
            name: None,
            roles: vec![],
            exp: i64::MAX,
        };
        assert!(!is_admin(&no_roles));
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    fn claims(sub: &str, admin: bool) -> AuthClaims {
        AuthClaims {
            sub: sub.to_string(),
            email: None,
            name: None,
            roles: if admin {
                vec!["admin".to_string()]
            } else {
                vec![]
            },
            exp: 0,
        }
    }

    #[test]
    fn granted_owner_cannot_revoke_or_downgrade_the_creator() {
        let co_owner = claims("co-owner", false);
        assert!(guard_canonical_owner(&co_owner, "creator", "creator", None).is_err());
        assert!(
            guard_canonical_owner(
                &co_owner,
                "creator",
                "creator",
                Some(&PermissionRole::Viewer)
            )
            .is_err()
        );
        // Re-affirming Owner is harmless.
        assert!(
            guard_canonical_owner(
                &co_owner,
                "creator",
                "creator",
                Some(&PermissionRole::Owner)
            )
            .is_ok()
        );
    }

    #[test]
    fn other_subjects_and_admins_are_unaffected() {
        let co_owner = claims("co-owner", false);
        assert!(guard_canonical_owner(&co_owner, "creator", "friend", None).is_ok());
        let admin = claims("root", true);
        assert!(guard_canonical_owner(&admin, "creator", "creator", None).is_ok());
    }

    #[test]
    fn subject_scope_is_none_only_for_admins() {
        assert_eq!(subject_scope(&claims("u1", false)), Some("u1"));
        assert_eq!(subject_scope(&claims("root", true)), None);
    }
}
