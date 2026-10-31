//! People and memberships (#147).
//!
//! SECURITY (owner decisions on #147):
//! - `/api/people/*` serves the caller's own registry only, with no admin
//!   bypass. Another owner's person is a 404, the same as a missing one.
//! - `GET /api/nodes/{id}/people` needs viewer access to the node and is
//!   redacted per viewer. The person's owner sees the card and the note;
//!   anyone else (a sharee, the admin) sees name and role only.
//! - Changing a node's people requires being the node's canonical owner
//!   (not just an editor, not the admin), and using a person you own. An
//!   editor can't put their own contacts onto someone else's node, since
//!   that would leak them to the node owner.
//! - Contact details are never logged.

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use common::{
    auth::AuthClaims,
    id::{MembershipId, NodeId, PersonId},
    ontology::Capability,
    person::{
        DuplicateWarning, MembershipInput, MembershipUpdate, MembershipView, Person, PersonBody,
        PersonUsage, RoleCount, email_keys, normalize, normalize_membership,
    },
};
use uuid::Uuid;

use crate::{
    auth::permissions::{require_editor, require_viewer},
    error::ApiError,
    routes::capability::refuse_unless,
    state::AppState,
};

/// `/api/people`
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_people).post(create_person))
        .route("/roles", get(role_suggestions))
        .route(
            "/{id}",
            get(get_person).put(update_person).delete(delete_person),
        )
        .route("/{id}/usages", get(person_usages))
}

/// `/api/nodes/{node_id}/people`
pub fn node_router() -> Router<AppState> {
    Router::new()
        .route("/", get(node_people).post(add_member))
        .route(
            "/{membership_id}",
            axum::routing::put(update_member).delete(remove_member),
        )
}

const LIST_DEFAULT: i64 = 100;
const LIST_MAX: i64 = 500;
const MAX_QUERY_CHARS: usize = 200;

#[derive(Debug, serde::Deserialize)]
struct ListParams {
    q: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct CreateParams {
    /// Save even though an email matches someone already in the registry.
    #[serde(default)]
    confirm: bool,
}

fn invalid(e: common::person::PersonError) -> ApiError {
    ApiError::Validation(e.to_string())
}

async fn list_people(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<Person>>, ApiError> {
    if params
        .q
        .as_ref()
        .is_some_and(|q| q.chars().count() > MAX_QUERY_CHARS)
    {
        return Err(ApiError::Validation("query too long".into()));
    }
    let limit = params.limit.unwrap_or(LIST_DEFAULT).clamp(1, LIST_MAX);
    let people = state
        .people
        .list(&claims.sub, params.q.as_deref(), limit)
        .await?;
    Ok(Json(people))
}

/// Create a person. If an email matches someone already in the caller's
/// registry, answer `409` with the matches (a duplicate warning, since email
/// is a signal and never an identity key) unless `?confirm=true`.
async fn create_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<CreateParams>,
    Json(body): Json<PersonBody>,
) -> Result<Response, ApiError> {
    let input = normalize(body.resolve(&[])).map_err(invalid)?;
    if !params.confirm {
        let matches = state
            .people
            .find_by_emails(&claims.sub, &email_keys(&input))
            .await?;
        if !matches.is_empty() {
            return Ok((StatusCode::CONFLICT, Json(DuplicateWarning { matches })).into_response());
        }
    }
    let person = state.people.create(&claims.sub, &input).await?;
    Ok((StatusCode::CREATED, Json(person)).into_response())
}

async fn get_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Person>, ApiError> {
    Ok(Json(state.people.get(&claims.sub, PersonId(id)).await?))
}

/// Replace a person's fields. Emails get the same duplicate warning as on
/// create, ignoring the person being edited: that's where the UI adds them,
/// since the create form only asks for a name.
async fn update_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Query(params): Query<CreateParams>,
    Json(body): Json<PersonBody>,
) -> Result<Response, ApiError> {
    let id = PersonId(id);
    // A pre-#155 tab doesn't send the address list: keep the stored one.
    // The lookup is owner-scoped, so another owner's id is still a 404.
    let stored = if body.is_legacy() {
        state.people.get(&claims.sub, id).await?.addresses
    } else {
        Vec::new()
    };
    let input = normalize(body.resolve(&stored)).map_err(invalid)?;
    if !params.confirm {
        let matches: Vec<_> = state
            .people
            .find_by_emails(&claims.sub, &email_keys(&input))
            .await?
            .into_iter()
            .filter(|m| m.id != id)
            .collect();
        if !matches.is_empty() {
            return Ok((StatusCode::CONFLICT, Json(DuplicateWarning { matches })).into_response());
        }
    }
    let person = state.people.update(&claims.sub, id, &input).await?;
    Ok(Json(person).into_response())
}

/// Deletes the person and, by cascade, their memberships.
async fn delete_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.people.delete(&claims.sub, PersonId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn person_usages(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<PersonUsage>>, ApiError> {
    Ok(Json(state.people.usages(&claims.sub, PersonId(id)).await?))
}

async fn role_suggestions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<RoleCount>>, ApiError> {
    Ok(Json(state.people.role_counts(&claims.sub).await?))
}

// ── Memberships ──────────────────────────────────────────────────────────────

/// A node's people, redacted for the caller.
async fn node_people(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
) -> Result<Json<Vec<MembershipView>>, ApiError> {
    let node = NodeId(node_id);
    require_viewer(state.permissions.as_ref(), &claims, node).await?;
    let members = state.people.memberships_for_node(node).await?;
    Ok(Json(
        members
            .into_iter()
            .map(|m| MembershipView::for_viewer(m, &claims.sub))
            .collect(),
    ))
}

/// Writes need the node's canonical owner: editor access first (403 for no
/// access at all), then ownership (403 for an editor who isn't the owner,
/// and for the admin).
async fn require_node_owner(
    state: &AppState,
    claims: &AuthClaims,
    node: NodeId,
) -> Result<common::node::Node, ApiError> {
    require_editor(state.permissions.as_ref(), claims, node).await?;
    let n = state.nodes.get(node).await?;
    if n.owner_id != claims.sub {
        return Err(ApiError::Forbidden(
            "only the node's owner can change its people".into(),
        ));
    }
    Ok(n)
}

async fn add_member(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(node_id): Path<Uuid>,
    Json(input): Json<MembershipInput>,
) -> Result<(StatusCode, Json<MembershipView>), ApiError> {
    let node = NodeId(node_id);
    let (role, note) = normalize_membership(input.role, input.note).map_err(invalid)?;
    let n = require_node_owner(&state, &claims, node).await?;
    // §3.2 (#216): only Projects have people. After the owner check, so
    // only the node's owner can learn its kind from this refusal. Existing
    // memberships elsewhere stay editable and removable.
    refuse_unless(&n.node_type, Capability::People)?;
    // The person must be the caller's own: another owner's id is a 404.
    state.people.get(&claims.sub, input.person_id).await?;
    let id = state
        .people
        .add_membership(node, input.person_id, role.as_deref(), note.as_deref())
        .await?;
    let view = membership_view(&state, node, id, &claims.sub).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn update_member(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((node_id, membership_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<MembershipUpdate>,
) -> Result<Json<MembershipView>, ApiError> {
    let (node, id) = (NodeId(node_id), MembershipId(membership_id));
    let (role, note) = normalize_membership(input.role, input.note).map_err(invalid)?;
    require_node_owner(&state, &claims, node).await?;
    state
        .people
        .update_membership(node, id, role.as_deref(), note.as_deref())
        .await?;
    Ok(Json(membership_view(&state, node, id, &claims.sub).await?))
}

async fn remove_member(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((node_id, membership_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let node = NodeId(node_id);
    require_node_owner(&state, &claims, node).await?;
    state
        .people
        .remove_membership(node, MembershipId(membership_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn membership_view(
    state: &AppState,
    node: NodeId,
    id: MembershipId,
    viewer: &str,
) -> Result<MembershipView, ApiError> {
    state
        .people
        .memberships_for_node(node)
        .await?
        .into_iter()
        .find(|m| m.id == id)
        .map(|m| MembershipView::for_viewer(m, viewer))
        .ok_or_else(|| ApiError::NotFound(format!("membership {id} not found")))
}
