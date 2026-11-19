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
    EmberTroveError,
    auth::AuthClaims,
    id::{MembershipId, NodeId, PersonId},
    mention::PersonMention,
    ontology::Capability,
    person::{
        DuplicateMatch, DuplicateWarning, MembershipInput, MembershipUpdate, MembershipView,
        Person, PersonBody, PersonUsage, RoleCount, normalize, normalize_membership,
    },
};
use uuid::Uuid;

use crate::{
    auth::permissions::{require_editor, require_viewer, subject_scope},
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
        .route("/{id}/mentions", get(person_mentions))
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

/// The refusal of a save that would give two of the caller's people one
/// email address (S8a; ONTOLOGY §7.1: the same address is the same
/// person): `409` with the caller's own people who have it. Always this
/// body, even with no match left to name: the app reads every 409 on a
/// person save as a `DuplicateWarning`.
fn collides(matches: Vec<DuplicateMatch>) -> Response {
    (StatusCode::CONFLICT, Json(DuplicateWarning { matches })).into_response()
}

/// A person write, or the 409 for the email rule.
///
/// The collisions are read first, to say who has the address, and nothing
/// is written when there is one. The database refuses a collision itself
/// (migration 056), so a save that slipped between that read and the write
/// comes back `AlreadyExists`: the collisions are read again and answered
/// the same way.
///
/// `?confirm=true` used to save anyway. It is ignored now: no route reads
/// it, so an old page's "Save anyway" gets the same 409 again.
async fn save_unless_it_collides<F>(
    state: &AppState,
    owner: &str,
    id: Option<PersonId>,
    input: &common::person::PersonInput,
    write: F,
    saved: StatusCode,
) -> Result<Response, ApiError>
where
    F: std::future::Future<Output = Result<Person, EmberTroveError>>,
{
    let matches = state
        .people
        .email_collisions(owner, id, &input.emails)
        .await?;
    if !matches.is_empty() {
        return Ok(collides(matches));
    }
    match write.await {
        Ok(person) => Ok((saved, Json(person)).into_response()),
        Err(EmberTroveError::AlreadyExists(_)) => Ok(collides(
            state
                .people
                .email_collisions(owner, id, &input.emails)
                .await?,
        )),
        Err(e) => Err(e.into()),
    }
}

/// Create a person. An email address that another of the caller's people
/// already lists refuses it: `409` with those people (see [`collides`]).
async fn create_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(body): Json<PersonBody>,
) -> Result<Response, ApiError> {
    let input = normalize(body.resolve(&[])).map_err(invalid)?;
    let write = state.people.create(&claims.sub, &input);
    save_unless_it_collides(
        &state,
        &claims.sub,
        None,
        &input,
        write,
        StatusCode::CREATED,
    )
    .await
}

async fn get_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Person>, ApiError> {
    Ok(Json(state.people.get(&claims.sub, PersonId(id)).await?))
}

/// Replace a person's fields. An address **added** here that another of
/// the caller's people lists refuses the save, as on create. One the
/// person already had never does: people who shared an address before the
/// rule stay editable (owner decision, 2026-11-18).
async fn update_person(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
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
    let write = state.people.update(&claims.sub, id, &input);
    save_unless_it_collides(&state, &claims.sub, Some(id), &input, write, StatusCode::OK).await
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

/// GET /people/{id}/mentions — "Mentioned in" (S9, ONTOLOGY §7.3): the
/// nodes whose body or notes mention this person.
///
/// The person must be the caller's own: another account's reads like a
/// missing one, as on every route here (`PersonRepo::get`). Only nodes
/// the caller can open are listed (§14), with no count of the rest; an
/// admin may open every node, and still only for their own person.
async fn person_mentions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<PersonMention>>, ApiError> {
    state.people.get(&claims.sub, PersonId(id)).await?;
    Ok(Json(
        state
            .mentions
            .mentioned_in(PersonId(id), subject_scope(&claims))
            .await?,
    ))
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
