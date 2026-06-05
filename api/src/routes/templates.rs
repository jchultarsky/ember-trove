use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use common::{
    auth::AuthClaims,
    id::TemplateId,
    template::{CreateTemplateRequest, NodeTemplate, UpdateTemplateRequest},
};
use garde::Validate;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_resource_owner},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_templates).post(create_template))
        .route(
            "/{id}",
            get(get_template)
                .put(update_template)
                .delete(delete_template),
        )
        .route("/{id}/set-default", put(set_default_template))
}

async fn list_templates(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<NodeTemplate>>, ApiError> {
    // SECURITY: scope to the caller's own templates; admins (None) see all.
    let subject = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };
    let templates = state.templates.list(subject).await?;
    Ok(Json(templates))
}

async fn get_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<NodeTemplate>, ApiError> {
    let template = state.templates.get(TemplateId(id)).await?;
    require_resource_owner(&claims, &template.created_by)?;
    Ok(Json(template))
}

async fn create_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<NodeTemplate>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let template = state.templates.create(&claims.sub, req).await?;
    Ok((StatusCode::CREATED, Json(template)))
}

async fn update_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTemplateRequest>,
) -> Result<Json<NodeTemplate>, ApiError> {
    let existing = state.templates.get(TemplateId(id)).await?;
    require_resource_owner(&claims, &existing.created_by)?;
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let template = state.templates.update(TemplateId(id), req).await?;
    Ok(Json(template))
}

async fn delete_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let existing = state.templates.get(TemplateId(id)).await?;
    require_resource_owner(&claims, &existing.created_by)?;
    state.templates.delete(TemplateId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Toggle the `is_default` flag for the given template.
///
/// Only the template's creator may change its default status.  Calling this
/// when the template is already the default will clear the flag (toggle off).
/// Calling it when it is not the default will set it and clear any sibling
/// default for the same `(created_by, node_type)` pair.
async fn set_default_template(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<NodeTemplate>, ApiError> {
    let template = state
        .templates
        .set_default(TemplateId(id), &claims.sub)
        .await?;
    Ok(Json(template))
}
