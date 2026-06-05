//! Backup / restore routes — all require admin role.
//!
//! Mounted under `/api/admin/backups` in `routes/mod.rs`.
//!
//! Trust model: admins are fully trusted to manage the entire system state.
//! Any admin may list, download, restore, or delete any other admin's
//! backup.  Backup/restore operations are categorically exempt from the
//! per-user rate limit — they may legitimately take longer than a normal
//! request, and the admin role is the only role that can repair the
//! system when something goes wrong.

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get, post},
};
use common::auth::AuthClaims;
use serde::Deserialize;
use uuid::Uuid;

/// Optional request body for creating a backup.
#[derive(Debug, Deserialize, Default)]
struct CreateBackupRequest {
    /// Optional user-provided comment describing the purpose of the backup.
    #[serde(default)]
    comment: Option<String>,
}

use crate::{auth::permissions::require_admin, backup as svc, error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_backups).post(create_backup_handler))
        .route("/{id}", delete(delete_backup_handler))
        .route("/{id}/download", get(download_backup_handler))
        .route("/{id}/preview", get(preview_restore_handler))
        .route("/{id}/restore", post(execute_restore_handler))
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn list_backups(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<common::backup::BackupJob>>, ApiError> {
    require_admin(&claims)?;
    // Any admin sees every backup — the role is trusted to operate on
    // the whole system, not just the backups they personally created.
    let jobs = state.backup.list_all().await.map_err(ApiError::from)?;
    Ok(Json(jobs))
}

async fn create_backup_handler(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    body: Option<Json<CreateBackupRequest>>,
) -> Result<(StatusCode, Json<common::backup::BackupJob>), ApiError> {
    require_admin(&claims)?;

    // No rate limit on admin-triggered backups — the admin role is
    // explicitly trusted, and backups may legitimately be run back-to-back
    // (e.g. before and after a risky migration).
    let comment = body.and_then(|b| b.0.comment);
    let job = svc::create_backup(&state, &claims.sub, comment.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(job)))
}

async fn delete_backup_handler(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_admin(&claims)?;

    // No per-creator check — any admin may delete any backup.
    let job = state.backup.get(id).await.map_err(ApiError::from)?;

    // Delete from S3 (best-effort).
    if let Err(e) = state.object_store.delete(&job.s3_key).await {
        tracing::warn!(key = %job.s3_key, "S3 delete during backup deletion failed: {e}");
    }

    state.backup.delete(id).await.map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn download_backup_handler(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    require_admin(&claims)?;

    // No per-creator check — any admin may download any backup.
    let job = state.backup.get(id).await.map_err(ApiError::from)?;

    let bytes = state
        .object_store
        .get(&job.s3_key)
        .await
        .map_err(|e| ApiError::Storage(format!("backup download failed: {e}")))?;

    let filename = format!("ember-trove-backup-{id}.tar.gz");
    let disposition = format!("attachment; filename=\"{filename}\"");

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .map_err(|e| ApiError::Internal(format!("response build failed: {e}")))
}

async fn preview_restore_handler(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<common::backup::BackupPreview>, ApiError> {
    require_admin(&claims)?;
    let preview = svc::preview_restore(&state, id, &claims.sub).await?;
    Ok(Json(preview))
}

async fn execute_restore_handler(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_admin(&claims)?;
    svc::execute_restore(&state, id, &claims.sub).await?;
    Ok(StatusCode::ACCEPTED)
}
