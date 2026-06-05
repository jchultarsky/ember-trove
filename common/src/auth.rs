use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Claims extracted from a validated JWT access token.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AuthClaims {
    /// OIDC subject identifier (unique user ID).
    pub sub: String,
    /// Email address (optional — depends on scope).
    pub email: Option<String>,
    /// Display name.
    pub name: Option<String>,
    /// Groups/roles from the identity provider (`cognito:groups` on Cognito).
    #[serde(default)]
    pub roles: Vec<String>,
    /// Token expiry (Unix timestamp).
    pub exp: i64,
}

/// Request body for `POST /auth/change-password`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Public user information returned by `GET /auth/me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub roles: Vec<String>,
}

impl From<AuthClaims> for UserInfo {
    fn from(claims: AuthClaims) -> Self {
        Self {
            sub: claims.sub,
            email: claims.email,
            name: claims.name,
            roles: claims.roles,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(sub: &str, email: Option<&str>, name: Option<&str>, roles: Vec<&str>) -> AuthClaims {
        AuthClaims {
            sub: sub.to_string(),
            email: email.map(str::to_string),
            name: name.map(str::to_string),
            roles: roles.into_iter().map(str::to_string).collect(),
            exp: 9_999_999_999,
        }
    }

    #[test]
    fn user_info_from_full_claims() {
        let c = claims(
            "user-1",
            Some("a@b.com"),
            Some("Alice"),
            vec!["admin", "user"],
        );
        let info = UserInfo::from(c);
        assert_eq!(info.sub, "user-1");
        assert_eq!(info.email.as_deref(), Some("a@b.com"));
        assert_eq!(info.name.as_deref(), Some("Alice"));
        assert_eq!(info.roles, vec!["admin", "user"]);
    }

    #[test]
    fn user_info_from_minimal_claims() {
        let c = claims("u2", None, None, vec![]);
        let info = UserInfo::from(c);
        assert_eq!(info.sub, "u2");
        assert!(info.email.is_none());
        assert!(info.name.is_none());
        assert!(info.roles.is_empty());
    }

    #[test]
    fn auth_claims_roles_default_empty() {
        // Deserialising a JWT payload without a `roles` key must produce an empty Vec.
        let json = r#"{"sub":"x","exp":1000000000}"#;
        let c: AuthClaims = serde_json::from_str(json).expect("deserialise");
        assert!(c.roles.is_empty());
    }

    #[test]
    fn auth_claims_round_trip_serialize() {
        let c = claims("sub42", Some("t@t.com"), None, vec!["user"]);
        let json = serde_json::to_string(&c).expect("serialize");
        let back: AuthClaims = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.sub, c.sub);
        assert_eq!(back.roles, c.roles);
    }
}
