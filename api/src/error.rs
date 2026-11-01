use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use common::EmberTroveError;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("internal error: {0}")]
    Internal(String),

    /// An upstream service (Cognito) failed after part of the work landed.
    /// The message is written for the client and names no internal detail;
    /// the upstream error itself is logged where it happened.
    #[error("upstream error: {0}")]
    Upstream(String),
}

impl From<EmberTroveError> for ApiError {
    fn from(err: EmberTroveError) -> Self {
        match err {
            EmberTroveError::NotFound(msg) => Self::NotFound(msg),
            EmberTroveError::AlreadyExists(msg) => Self::Conflict(msg),
            EmberTroveError::Unauthorized(msg) => Self::Unauthorized(msg),
            EmberTroveError::Forbidden(msg) => Self::Forbidden(msg),
            EmberTroveError::Validation(msg) => Self::Validation(msg),
            EmberTroveError::PreconditionFailed(msg) => Self::PreconditionFailed(msg),
            EmberTroveError::Internal(msg) => Self::Internal(msg),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            Self::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            Self::PreconditionFailed(msg) => (StatusCode::PRECONDITION_FAILED, msg.clone()),
            Self::Database(err) => {
                tracing::error!(%err, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database error".to_string(),
                )
            }
            Self::Storage(msg) => {
                tracing::error!(%msg, "storage error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage error".to_string(),
                )
            }
            Self::Upstream(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            Self::Internal(msg) => {
                tracing::error!(%msg, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
