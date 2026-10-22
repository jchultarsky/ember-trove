use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, put},
};
use common::{
    auth::AuthClaims,
    editor_pref::{EditorPref, SetCollapsedRequest, SetEditorPrefRequest},
};
use garde::Validate;

use crate::{error::ApiError, state::AppState};

/// Mounts under `/editor-prefs`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_prefs).put(set_pref))
        .route("/collapsed", put(set_collapsed))
}

/// GET /editor-prefs — all of the caller's saved editor prefs (heights and
/// note collapse state).
async fn list_prefs(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<EditorPref>>, ApiError> {
    let prefs = state.editor_prefs.list_for_owner(&claims.sub).await?;
    Ok(Json(prefs))
}

/// PUT /editor-prefs — upsert one entity's prefs; only the fields sent change.
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
    if req.height.is_none() && req.collapsed.is_none() {
        return Err(ApiError::Validation(
            "provide height and/or collapsed".to_string(),
        ));
    }
    if req.collapsed.is_some() && req.entity_kind != "note" {
        return Err(ApiError::Validation(
            "collapsed applies to notes only".to_string(),
        ));
    }
    // Owner-scoped: prefs are keyed by the caller's sub, so a request can only
    // ever read or write the caller's own rows.
    state
        .editor_prefs
        .set(
            &claims.sub,
            &req.entity_kind,
            req.entity_id,
            req.height,
            req.collapsed,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /editor-prefs/collapsed — collapse or expand many notes in one request
/// (Collapse all / Expand all), capped at `MAX_BULK_COLLAPSE` ids.
async fn set_collapsed(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<SetCollapsedRequest>,
) -> Result<StatusCode, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if req.entity_kind != "note" {
        return Err(ApiError::Validation(
            "collapsed applies to notes only".to_string(),
        ));
    }
    state
        .editor_prefs
        .set_collapsed_many(
            &claims.sub,
            &req.entity_kind,
            &req.entity_ids,
            req.collapsed,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
