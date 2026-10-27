use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmberTroveError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("already exists: {0}")]
    AlreadyExists(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("validation error: {0}")]
    Validation(String),

    /// A write's precondition no longer holds (the resource changed since
    /// the client read it); nothing was written. HTTP 412.
    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("internal error: {0}")]
    Internal(String),
}
