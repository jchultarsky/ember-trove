use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::get,
};
use common::{auth::AuthClaims, editor_pref::{EditorPref, SetEditorPrefRequest}};
use garde::Validate;

use crate::{error::ApiError, state::AppState};

/// Mounts under `/editor-prefs`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_prefs).put(set_pref))
}

/// GET /editor-prefs — all of the caller's saved editor heights.
async fn list_prefs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<EditorPref>>, ApiError> {
    let prefs = state.editor_prefs.list_for_owner(&claims.sub).await?;
    Ok(Json(prefs))
}

/// PUT /editor-prefs — upsert one entity's editor height.
async fn set_pref(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<SetEditorPrefRequest>,
) -> Result<StatusCode, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.entity_kind != "task" && req.entity_kind != "note" {
        return Err(ApiError::Validation(
            "entity_kind must be 'task' or 'note'".to_string(),
        ));
    }
    state
        .editor_prefs
        .set(&claims.sub, &req.entity_kind, req.entity_id, req.height)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
