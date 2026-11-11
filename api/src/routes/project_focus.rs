//! Focus (#282, proposal B): `/api/focus`, the projects that hold the
//! caller's attention (`common::project_focus`).
//!
//! SECURITY:
//! - Focus is private to its person, with no admin bypass (owner decision,
//!   2026-11-09). Every handler passes `claims.sub` and the repo matches
//!   it, so nobody reads or changes another account's Focus or why-line,
//!   an admin included. There is no route that takes an account.
//! - Putting a project in Focus needs viewer rights on it. A project the
//!   caller can't see answers exactly like one that doesn't exist (404),
//!   and that check comes before the kind (422) and the why-line (422), so
//!   neither can tell a stranger anything about a node.
//! - The reads check the rights again, in the query: a project whose share
//!   was taken back drops out, title and all.
//! - Removing, reordering and changing a why-line act on the caller's own
//!   rows only and never look at the node, so their answers say nothing
//!   about it.
//! - Focus writes no History entry and fires no webhook: both are shared
//!   with the node, and Focus is not.
//! - The why-line is user content: never logged.

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use common::{
    auth::AuthClaims,
    id::NodeId,
    ontology::Capability,
    project_focus::{
        FocusEntry, FocusError, FocusInput, FocusItem, FocusOrder, FocusWhy, MAX_ORDER,
        holds_focus, normalize_why,
    },
};
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, or_missing, or_missing_row, require_viewer},
    error::ApiError,
    routes::{capability::refuse_unless, tasks::dashboard_entries},
    state::AppState,
};

/// What a refused or absent project is called in a 404.
const PROJECT: &str = "project";

/// `/api/focus`
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_focus))
        // A fixed segment wins over `{node_id}`, so "order" is never read
        // as a node.
        .route("/order", put(set_order))
        .route(
            "/{node_id}",
            get(get_focus).put(put_focus).delete(remove_focus),
        )
        .route("/{node_id}/why", put(set_why))
}

/// The caller's Focus, in their order, each entry with its dashboard card.
async fn list_focus(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<FocusEntry>>, ApiError> {
    let focused = state.focus.list(&claims.sub, is_admin(&claims)).await?;
    let (items, nodes): (Vec<FocusItem>, Vec<_>) =
        focused.into_iter().map(|f| (f.item, f.node)).unzip();
    // The cards are matched to the entries by node, not by position: the
    // Focus order is the entries', whatever order the cards come back in.
    let mut cards: HashMap<NodeId, _> = dashboard_entries(&state, &claims, nodes)
        .await?
        .into_iter()
        .map(|card| (card.node_id, card))
        .collect();
    Ok(Json(
        items
            .into_iter()
            .filter_map(|focus| {
                let project = cards.remove(&focus.node_id)?;
                Some(FocusEntry { focus, project })
            })
            .collect(),
    ))
}

/// The caller's entry for this project, or `null`. Anything that is not in
/// their Focus reads the same: a project they can't see, one that doesn't
/// exist, one they never added.
async fn get_focus(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Option<FocusItem>>, ApiError> {
    Ok(Json(
        state
            .focus
            .get(&claims.sub, is_admin(&claims), NodeId(node_id))
            .await?,
    ))
}

/// Put the project in the caller's Focus, or change its why-line. A body
/// without `why_now` leaves the why-line alone (`FocusInput`).
async fn put_focus(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(input): Json<FocusInput>,
) -> Result<Json<FocusItem>, ApiError> {
    let node_id = NodeId(node_id);
    // Rights first. The caller can't see the node, so a refusal is never a
    // 403: that would say the id is real.
    let check = require_viewer(state.permissions.as_ref(), &claims, node_id).await;
    or_missing(check, false, PROJECT)?;
    let node = state
        .nodes
        .get(node_id)
        .await
        .map_err(or_missing_row(PROJECT))?;
    refuse_unless(&node.node_type, Capability::Focus)?;
    if !holds_focus(&node.node_type, &node.status) {
        // The read would leave it out, so adding it would do nothing.
        return Err(ApiError::Validation(FocusError::Archived.to_string()));
    }
    let why = input
        .change()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let item = state
        .focus
        .put(&claims.sub, node_id, &why)
        .await
        .map_err(or_missing_row(PROJECT))?;
    Ok(Json(item))
}

/// Change the why-line of a project that is in the caller's Focus. It
/// never adds one: a project that isn't there is a 404, so a why-line saved
/// from a page that is out of date can't put a removed project back.
///
/// SECURITY: this acts on the caller's own row and never looks at the node,
/// so the 404 says only "not in your Focus", the same for a hidden node, a
/// missing one and one never added. The why-line is checked first, so a
/// refusal of it says nothing either.
async fn set_why(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(body): Json<FocusWhy>,
) -> Result<Json<FocusItem>, ApiError> {
    let why =
        normalize_why(body.why_now.as_deref()).map_err(|e| ApiError::Validation(e.to_string()))?;
    let item = state
        .focus
        .set_why(&claims.sub, NodeId(node_id), why.as_deref())
        .await
        .map_err(or_missing_row(PROJECT))?;
    Ok(Json(item))
}

/// Set the order of the caller's Focus.
async fn set_order(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(order): Json<FocusOrder>,
) -> Result<StatusCode, ApiError> {
    if order.node_ids.len() > MAX_ORDER {
        return Err(ApiError::Validation(FocusError::OrderTooLong.to_string()));
    }
    state.focus.reorder(&claims.sub, &order.node_ids).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Take the project out of the caller's Focus. 204 whether or not it was
/// there: the row is theirs alone, and the answer says nothing about the
/// node.
async fn remove_focus(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.focus.remove(&claims.sub, NodeId(node_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
