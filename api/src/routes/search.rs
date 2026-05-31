use axum::{Extension, Json, Router, extract::State, routing::get};
use common::{
    auth::AuthClaims,
    search::{SearchQuery, SearchResponse},
};

use crate::{auth::permissions::is_admin, error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(search))
}

async fn search(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    // SECURITY: scope results to the caller's own content. Admins (subject =
    // None) search across all owners; everyone else is restricted to their sub.
    let subject = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };
    let response = state.search.search(&query, subject).await?;
    Ok(Json(response))
}
