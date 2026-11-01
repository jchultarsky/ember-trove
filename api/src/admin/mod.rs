pub mod cognito;

use async_trait::async_trait;
use common::admin::{AdminUser, CreateAdminUserRequest};

pub use cognito::CognitoAdminClient;

use crate::error::ApiError;

/// The account directory the admin routes manage (#239): Cognito in
/// production, a recording fake in the route tests.
///
/// `username` is the Cognito username, which is the account's `sub` (the
/// pool signs in by email, so Cognito generates a UUID username). An
/// unknown user is `ApiError::NotFound`; every other failure is logged
/// server-side and returned as a generic error (see `cognito_err`).
#[async_trait]
pub trait UserDirectory: Send + Sync {
    /// Every user in the pool, with their groups.
    async fn list_users(&self) -> Result<Vec<AdminUser>, ApiError>;
    /// Create a user, assign its first groups and optionally email it.
    async fn create_user(&self, req: &CreateAdminUserRequest) -> Result<AdminUser, ApiError>;
    /// The account's email, or `None` when the pool no longer has it.
    async fn email_of(&self, username: &str) -> Result<Option<String>, ApiError>;
    /// The user with this email, if any.
    async fn find_user_by_email(&self, email: &str) -> Result<Option<AdminUser>, ApiError>;
    /// Hard-delete the account.
    async fn delete_user(&self, username: &str) -> Result<(), ApiError>;
    /// Every group (role) in the pool.
    async fn list_groups(&self) -> Result<Vec<String>, ApiError>;
    /// Replace the user's group membership with `desired`.
    async fn set_user_groups(&self, username: &str, desired: &[String]) -> Result<(), ApiError>;
    /// Block sign-in (and session renewal) for the account.
    async fn disable_user(&self, username: &str) -> Result<(), ApiError>;
    /// Allow sign-in again.
    async fn enable_user(&self, username: &str) -> Result<(), ApiError>;
    /// Revoke every refresh token the account holds, so no session renews.
    async fn sign_out_everywhere(&self, username: &str) -> Result<(), ApiError>;
}
