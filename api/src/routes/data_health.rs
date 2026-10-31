//! `GET /api/data-health` (#208): the caller's data that breaks the data
//! model (`docs/ONTOLOGY.md`), per rule. Read-only, computed on read.
//!
//! SECURITY (§14): the report is always the caller's own data, admins
//! included. It uses `claims.sub`, never `subject_scope` (which is `None`,
//! "everything", for an admin).

use axum::{Extension, Json, Router, extract::State, routing::get};
use common::{auth::AuthClaims, data_health::DataHealthReport};

use crate::{error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(report))
}

async fn report(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<DataHealthReport>, ApiError> {
    let rules = state.data_health.report_for(&claims.sub).await?;
    Ok(Json(DataHealthReport { rules }))
}
