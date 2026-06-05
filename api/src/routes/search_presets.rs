use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use common::{
    auth::AuthClaims,
    id::SearchPresetId,
    search::{CreateSearchPresetRequest, SearchPreset},
};
use garde::Validate;
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_presets).post(create_preset))
        .route("/{id}", axum::routing::delete(delete_preset))
}

async fn list_presets(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<SearchPreset>>, ApiError> {
    let presets = state.search_presets.list(&claims.sub).await?;
    Ok(Json(presets))
}

async fn create_preset(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateSearchPresetRequest>,
) -> Result<(StatusCode, Json<SearchPreset>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let preset = state.search_presets.create(&claims.sub, req).await?;
    Ok((StatusCode::CREATED, Json(preset)))
}

async fn delete_preset(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .search_presets
        .delete(SearchPresetId(id), &claims.sub)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
