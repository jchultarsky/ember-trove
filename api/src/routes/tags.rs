use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use common::{
    auth::AuthClaims,
    id::TagId,
    tag::{CreateTagRequest, Tag, UpdateTagRequest},
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, or_missing, or_missing_row, require_resource_owner},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tags).post(create_tag))
        .route("/{id}", put(update_tag).delete(delete_tag))
}

async fn list_tags(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<Tag>>, ApiError> {
    // Admins see all tags; regular users see only their own.
    let tags = if is_admin(&claims) {
        state.tags.list_all().await?
    } else {
        state.tags.list(&claims.sub).await?
    };
    Ok(Json(tags))
}

async fn create_tag(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateTagRequest>,
) -> Result<(StatusCode, Json<Tag>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let tag = state.tags.create(&claims.sub, req).await?;
    Ok((StatusCode::CREATED, Json(tag)))
}

/// What a tag that isn't there, or isn't the caller's, is called.
const TAG: &str = "tag";

async fn update_tag(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTagRequest>,
) -> Result<Json<Tag>, ApiError> {
    // Only the tag owner or an admin may update.
    let existing = state
        .tags
        .get(TagId(id))
        .await
        .map_err(or_missing_row(TAG))?;
    // A tag is seen by its owner only: anyone else's reads as missing.
    or_missing(
        require_resource_owner(&claims, &existing.owner_id),
        false,
        TAG,
    )?;
    let tag = state.tags.update(TagId(id), req).await?;
    Ok(Json(tag))
}

async fn delete_tag(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let existing = state
        .tags
        .get(TagId(id))
        .await
        .map_err(or_missing_row(TAG))?;
    or_missing(
        require_resource_owner(&claims, &existing.owner_id),
        false,
        TAG,
    )?;
    state.tags.delete(TagId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
