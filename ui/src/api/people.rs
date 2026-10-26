//! People and memberships (#147).

use common::{
    id::{MembershipId, NodeId, PersonId},
    person::{
        DuplicateWarning, MembershipInput, MembershipUpdate, MembershipView, Person, PersonInput,
        PersonUsage, RoleCount,
    },
};
use gloo_net::http::Request;

use super::{api_url, delete_empty, get_json, parse_json, post_json, put_json};
use crate::error::UiError;

/// The caller's people; `q` filters on name, company, email and phone.
pub async fn fetch_people(q: Option<&str>) -> Result<Vec<Person>, UiError> {
    match q.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            get_json(&format!(
                "/people?limit=500&q={}",
                js_sys::encode_uri_component(q)
            ))
            .await
        }
        None => get_json("/people?limit=500").await,
    }
}

/// At most `limit` of the caller's people matching `q` (global search).
pub async fn search_people(q: &str, limit: u32) -> Result<Vec<Person>, UiError> {
    get_json(&format!(
        "/people?limit={limit}&q={}",
        js_sys::encode_uri_component(q.trim())
    ))
    .await
}

pub async fn fetch_person(id: PersonId) -> Result<Person, UiError> {
    get_json(&format!("/people/{id}")).await
}

/// What saving a person (create or update) did: saved it, or found that an
/// email is already in the registry (a warning; retry with `confirm`).
pub enum SavePerson {
    Saved(Person),
    Duplicate(DuplicateWarning),
}

pub async fn create_person(input: &PersonInput, confirm: bool) -> Result<SavePerson, UiError> {
    let path = if confirm {
        "/people?confirm=true"
    } else {
        "/people"
    };
    let resp = Request::post(&api_url(path))
        .json(input)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    if resp.status() == 409 {
        let warning = resp
            .json::<DuplicateWarning>()
            .await
            .map_err(|e| UiError::Parse(e.to_string()))?;
        return Ok(SavePerson::Duplicate(warning));
    }
    parse_json(resp).await.map(SavePerson::Saved)
}

/// Save a person. Like create, an email someone else in the registry has
/// is a warning first; `confirm` saves anyway.
pub async fn update_person(
    id: PersonId,
    input: &PersonInput,
    confirm: bool,
) -> Result<SavePerson, UiError> {
    let path = if confirm {
        format!("/people/{id}?confirm=true")
    } else {
        format!("/people/{id}")
    };
    let resp = Request::put(&api_url(&path))
        .json(input)
        .map_err(|e| UiError::Parse(e.to_string()))?
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    if resp.status() == 409 {
        let warning = resp
            .json::<DuplicateWarning>()
            .await
            .map_err(|e| UiError::Parse(e.to_string()))?;
        return Ok(SavePerson::Duplicate(warning));
    }
    parse_json(resp).await.map(SavePerson::Saved)
}

pub async fn delete_person(id: PersonId) -> Result<(), UiError> {
    delete_empty(&format!("/people/{id}")).await
}

pub async fn fetch_person_usages(id: PersonId) -> Result<Vec<PersonUsage>, UiError> {
    get_json(&format!("/people/{id}/usages")).await
}

pub async fn fetch_role_suggestions() -> Result<Vec<RoleCount>, UiError> {
    get_json("/people/roles").await
}

pub async fn fetch_node_people(node: NodeId) -> Result<Vec<MembershipView>, UiError> {
    get_json(&format!("/nodes/{node}/people")).await
}

pub async fn add_node_person(
    node: NodeId,
    input: &MembershipInput,
) -> Result<MembershipView, UiError> {
    post_json(&format!("/nodes/{node}/people"), input).await
}

pub async fn update_node_person(
    node: NodeId,
    id: MembershipId,
    input: &MembershipUpdate,
) -> Result<MembershipView, UiError> {
    put_json(&format!("/nodes/{node}/people/{id}"), input).await
}

pub async fn remove_node_person(node: NodeId, id: MembershipId) -> Result<(), UiError> {
    delete_empty(&format!("/nodes/{node}/people/{id}")).await
}
