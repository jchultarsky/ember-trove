use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
};
use bytes::Bytes;
use common::{
    activity::{ActivityAction, ActivityEntry},
    attachment::Attachment,
    auth::AuthClaims,
    edge::{EdgeType, EdgeWithTitles, Home, HomeNode, NodeHome, SetHomeRequest},
    id::{NodeId, NodeVersionId, PermissionId, TagId},
    node::{
        CreateNodeRequest, Node, NodeListParams, NodeListResponse, NodeLookupHit, NodeLookupParams,
        NodeTitleEntry, ProjectStatusCount, UpdateNodeRequest,
    },
    node_version::NodeVersion,
    ontology::{Capability, allows, edge_kind_violation},
    permission::{GrantPermissionRequest, InviteRequest, Permission, PermissionRole},
    project_status,
    tag::Tag,
    webhook::{EVENT_NODE_CREATED, EVENT_NODE_DELETED, EVENT_NODE_UPDATED},
};
use garde::Validate;
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::permissions::{
        guard_canonical_owner, is_admin, require_editor, require_owner, require_viewer,
        subject_scope,
    },
    error::ApiError,
    notify::maybe_notify_invite,
    repo::activity::ActivityActor,
    routes::capability::{refuse_unless, require_capability},
    state::AppState,
    webhook_dispatch::dispatch,
};
use common::wikilink::parse_wikilink_titles;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_nodes).post(create_node))
        .route("/titles", get(list_titles))
        .route("/lookup", get(lookup_titles))
        .route("/resolve-titles", get(resolve_titles))
        .route("/project-statuses", get(project_statuses))
        .route("/slug/{slug}", get(get_node_by_slug))
        .route("/{id}", get(get_node).put(update_node).delete(delete_node))
        .route("/{id}/neighbors", get(neighbors))
        .route("/{id}/backlinks", get(backlinks))
        .route("/{id}/edges", get(list_edges_for_node))
        .route("/{id}/home", get(get_home).put(set_home))
        .route("/{id}/tags", get(list_tags_for_node))
        .route("/{id}/tags/{tag_id}", post(attach_tag).delete(detach_tag))
        .route(
            "/{id}/attachments",
            get(list_attachments)
                .post(upload_attachment)
                .layer(DefaultBodyLimit::max(50 * 1024 * 1024)), // 50 MiB upload cap
        )
        .route(
            "/{id}/permissions",
            get(list_permissions).post(grant_permission),
        )
        .route("/{id}/permissions/{perm_id}", delete(revoke_permission))
        .route("/{id}/invite", post(invite))
        .route("/{id}/export", get(export_node))
        .route("/{id}/activity", get(list_activity))
        .route("/{id}/versions", get(list_versions))
        .route("/{id}/versions/{version_id}/restore", post(restore_version))
        .route("/{id}/duplicate", post(duplicate_node))
        .route("/{id}/pin", axum::routing::put(set_pin))
}

#[derive(serde::Deserialize)]
struct SetPinRequest {
    pinned: bool,
}

/// `PUT /api/nodes/:id/pin` — toggle the pinned flag.  Used by the
/// v2.9.0 dashboard pin button.  Only the node owner / admin / editor
/// can set pin state.
async fn set_pin(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<SetPinRequest>,
) -> Result<Json<Node>, ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let node = state.nodes.set_pinned(NodeId(id), req.pinned).await?;
    Ok(Json(node))
}

// ── Node list ────────────────────────────────────────────────────────────────

async fn list_nodes(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(mut params): Query<NodeListParams>,
) -> Result<Json<NodeListResponse>, ApiError> {
    // Enforce private-by-default: only show nodes the caller owns or has a
    // permission row for.  The client-supplied owner_id filter is ignored.
    // Admins bypass the permission filter and can see all nodes.
    params.owner_id = None;
    if !is_admin(&claims) {
        params.subject_id = Some(claims.sub.clone());
    }

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(200);
    let (nodes, total) = state.nodes.list(params).await?;
    // `has_more` was computed as `page*per_page + nodes.len() < total`, which
    // double-counts the current page (returns false on page=1 of 57 with
    // per_page=50, hiding 7 nodes from the graph view). Correct check: are
    // there any rows beyond what's been seen so far?
    let seen = (page as u64 - 1) * (per_page as u64) + (nodes.len() as u64);
    let has_more = seen < total;

    Ok(Json(NodeListResponse {
        nodes,
        total,
        page,
        per_page,
        has_more,
    }))
}

// ── Title list (used for wiki-link autocomplete) ─────────────────────────────

async fn list_titles(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<NodeTitleEntry>>, ApiError> {
    // Admins see every title (bypass).  Non-admins only see titles of
    // nodes they own or have been explicitly granted a permission on —
    // otherwise wiki-link autocomplete would enumerate every node in the
    // system, including nodes never shared with the caller.
    let subject = if is_admin(&claims) {
        None
    } else {
        Some(claims.sub.as_str())
    };
    let titles = state.nodes.list_titles(subject).await?;
    Ok(Json(titles))
}

// ── Title lookup (node pickers) ──────────────────────────────────────────────

/// Upper bounds for `GET /nodes/lookup`: a picker shows a handful of rows,
/// and the query is a title fragment, not a document.
const LOOKUP_DEFAULT_LIMIT: u32 = 8;
const LOOKUP_MAX_LIMIT: u32 = 20;
const LOOKUP_MAX_QUERY_CHARS: usize = 200;

/// `GET /nodes/lookup?q=&limit=&node_type=` — title matches for the node pickers.
/// Same visibility as `/nodes/titles` (own + shared; admins everything).
async fn lookup_titles(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<NodeLookupParams>,
) -> Result<Json<Vec<NodeLookupHit>>, ApiError> {
    let q = params.q.trim();
    if q.chars().count() > LOOKUP_MAX_QUERY_CHARS {
        return Err(ApiError::Validation("query too long".to_string()));
    }
    let limit = params
        .limit
        .unwrap_or(LOOKUP_DEFAULT_LIMIT)
        .clamp(1, LOOKUP_MAX_LIMIT);
    let hits = state
        .nodes
        .lookup_titles(
            subject_scope(&claims),
            q,
            i64::from(limit),
            params.node_type.as_ref(),
        )
        .await?;
    Ok(Json(hits))
}

// ── Wiki-link resolution (render-time) ──────────────────────────────────────

/// At most this many titles per request; a body rarely links more, and the
/// UI batches beyond it.
const RESOLVE_MAX_TITLES: usize = 100;
const RESOLVE_MAX_TITLE_CHARS: usize = 300;

#[derive(Debug, serde::Deserialize)]
struct ResolveTitlesParams {
    /// A JSON array of titles. A GET (reads aren't write-rate-limited) with
    /// one parameter avoids splitting titles that contain commas.
    titles: String,
}

/// `GET /nodes/resolve-titles?titles=["A","b"]` → `{ "a": id, "b": id }`
/// (keys lowercased; unmatched titles absent). Lets a node page resolve
/// just the `[[links]]` its body contains instead of downloading every
/// title (#134).
///
/// SECURITY: resolves among nodes visible to the CALLER only (their own
/// first, then ones shared with them) via `find_ids_by_titles`, the same
/// rule as wiki-link edge sync since v2.31.1 — never another owner's
/// private node. Bounded: ≤100 titles of ≤300 chars.
async fn resolve_titles(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Query(params): Query<ResolveTitlesParams>,
) -> Result<Json<std::collections::HashMap<String, NodeId>>, ApiError> {
    let titles: Vec<String> = serde_json::from_str(&params.titles)
        .map_err(|_| ApiError::Validation("titles must be a JSON array of strings".to_string()))?;
    if titles.len() > RESOLVE_MAX_TITLES {
        return Err(ApiError::Validation(format!(
            "at most {RESOLVE_MAX_TITLES} titles per request"
        )));
    }
    let mut lowered: Vec<String> = Vec::with_capacity(titles.len());
    for t in titles {
        let t = t.trim().to_lowercase();
        if t.chars().count() > RESOLVE_MAX_TITLE_CHARS {
            return Err(ApiError::Validation("title too long".to_string()));
        }
        if !t.is_empty() && !lowered.contains(&t) {
            lowered.push(t);
        }
    }
    if lowered.is_empty() {
        return Ok(Json(std::collections::HashMap::new()));
    }
    let map = state
        .nodes
        .find_ids_by_titles(&claims.sub, &lowered)
        .await?;
    Ok(Json(map))
}

// ── Project status (#143) ────────────────────────────────────────────────────

/// `GET /nodes/project-statuses` — the statuses the caller already uses on
/// their projects, most-used first: the status picker's suggestions.
///
/// SECURITY: the caller's OWN nodes only — not nodes shared with them, and
/// not everyone's for the admin. A status is free text ("Acquisition:
/// Physna") and mustn't leak across owners.
async fn project_statuses(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<Json<Vec<ProjectStatusCount>>, ApiError> {
    Ok(Json(state.nodes.project_status_counts(&claims.sub).await?))
}

fn status_error(e: project_status::StatusError) -> ApiError {
    ApiError::Validation(e.to_string())
}

/// A new node's project status. A given one is validated (blank = none).
/// Otherwise a project takes the status token from its starting body (a
/// template's `## Status` section, moved out so there's one copy), or
/// starts as "Active", which is what the old scaffold said. Other types
/// get none, and a status given for one is refused (§3.2, #216); a blank
/// one is no status, so it passes.
fn initial_project_status(req: &mut CreateNodeRequest) -> Result<(), ApiError> {
    if let Some(given) = req.project_status.take() {
        req.project_status = project_status::normalize(&given).map_err(status_error)?;
        if req.project_status.is_some() {
            refuse_unless(&req.node_type, Capability::ProjectStatus)?;
        }
        return Ok(());
    }
    if !allows(&req.node_type, Capability::ProjectStatus) {
        return Ok(());
    }
    match req
        .body
        .as_deref()
        .and_then(project_status::split_from_body)
    {
        Some(split) => {
            req.body = Some(split.body);
            req.project_status = Some(split.status);
        }
        None => req.project_status = Some(project_status::DEFAULT.to_string()),
    }
    Ok(())
}

// ── Node CRUD ────────────────────────────────────────────────────────────────

async fn create_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(mut req): Json<CreateNodeRequest>,
) -> Result<(StatusCode, Json<Node>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    initial_project_status(&mut req)?;

    let template_id = req.template_id;
    let node = state.nodes.create(&claims.sub, req).await?;

    // Auto-grant Owner permission for the creator so that require_role()
    // checks work immediately from the first request.
    let owner_req = GrantPermissionRequest {
        subject_id: claims.sub.clone(),
        role: PermissionRole::Owner,
    };
    state
        .permissions
        .grant(node.id, &claims.sub, owner_req)
        .await
        .map_err(|e| ApiError::Internal(format!("auto-grant owner permission failed: {e}")))?;

    sync_wikilinks(
        &state,
        node.id,
        &node.owner_id,
        node.body.as_deref().unwrap_or(""),
    )
    .await?;
    let (action, mut meta) = if let Some(tid) = template_id {
        (
            ActivityAction::CreatedFromTemplate,
            json!({ "title": node.title, "template_id": tid }),
        )
    } else {
        (ActivityAction::Created, json!({ "title": node.title }))
    };
    with_initial_status(&mut meta, node.project_status.as_deref());
    log_activity(&state, node.id, &claims, action, meta).await;
    dispatch(
        state.webhooks.clone(),
        EVENT_NODE_CREATED,
        Some(node.id),
        &claims.sub,
        &node.owner_id,
    );
    Ok((StatusCode::CREATED, Json(node)))
}

async fn get_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Node>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let node = state.nodes.get(NodeId(id)).await?;
    Ok(Json(node))
}

async fn get_node_by_slug(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(slug): Path<String>,
) -> Result<Json<Node>, ApiError> {
    // Resolve slug → id first so we can check permissions.
    let node = state.nodes.get_by_slug(&slug).await?;
    require_viewer(state.permissions.as_ref(), &claims, node.id).await?;
    Ok(Json(node))
}

/// How long after an `Edited` activity entry further edits by the same user
/// are coalesced into it instead of logged again. Editor autosave PATCHes
/// every few seconds of idle time; one "Edited" line per editing session is
/// what the dashboard recap should show. (2026-06-10)
const EDIT_ACTIVITY_COALESCE_MINUTES: i64 = 15;

/// Whether a body snapshot should be recorded in `node_versions`.
///
/// Skips the insert when the new body is identical to the latest stored
/// version — title/status-only saves and autosave repeats would otherwise
/// pile up duplicate rows.
fn version_snapshot_needed(latest_body: Option<&str>, new_body: &str) -> bool {
    latest_body != Some(new_body)
}

/// Whether an `Edited` activity entry should be logged, or coalesced into
/// `latest`, the node's last *Edited* entry (same subject, still inside the
/// coalescing window).
fn edit_activity_needed(
    latest: Option<&ActivityEntry>,
    subject: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let Some(e) = latest else { return true };
    e.subject_id != subject
        || now - e.created_at >= chrono::TimeDelta::minutes(EDIT_ACTIVITY_COALESCE_MINUTES)
}

/// History starts at creation (#208): a node created with a project status
/// records it on its `created` entry, so the first `status_changed` has a
/// visible "from".
fn with_initial_status(meta: &mut serde_json::Value, status: Option<&str>) {
    if let (Some(obj), Some(s)) = (meta.as_object_mut(), status) {
        obj.insert("project_status".to_string(), json!(s));
    }
}

/// The status a duplicate of `src` starts with: a copy never carries a
/// status its kind may not have (§3.2, #216), even when the original does.
fn copied_status(src: &Node) -> Option<String> {
    src.project_status
        .clone()
        .filter(|_| allows(&src.node_type, Capability::ProjectStatus))
}

/// Whether an update changes the project status and nothing else: what the
/// status chip sends.
fn is_status_only(req: &UpdateNodeRequest) -> bool {
    req.project_status.is_some()
        && req.title.is_none()
        && req.body.is_none()
        && req.metadata.is_none()
        && req.status.is_none()
}

async fn update_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(mut req): Json<UpdateNodeRequest>,
) -> Result<Json<Node>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    // A set project status is validated; blank means clear (#143).
    if let Some(Some(raw)) = &req.project_status {
        req.project_status = Some(project_status::normalize(raw).map_err(status_error)?);
    }
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    // §3.2 (#216): only a Project's status may be *set*. Clearing is always
    // allowed (it's the Data health fix), and saves that leave the status
    // alone (every autosave) pay no extra lookup.
    if matches!(req.project_status, Some(Some(_))) {
        require_capability(&state, NodeId(id), Capability::ProjectStatus).await?;
    }
    let status_only = is_status_only(&req);
    let node = state
        .nodes
        .update(NodeId(id), req, &ActivityActor::from_claims(&claims))
        .await?;
    sync_wikilinks(
        &state,
        node.id,
        &node.owner_id,
        node.body.as_deref().unwrap_or(""),
    )
    .await?;
    // Record a body snapshot unless identical to the latest stored version
    // (autosave repeats and title/status-only saves). Awaited inline — the
    // previous fire-and-forget spawn could write snapshots out of order when
    // saves arrive seconds apart. Failure stays non-fatal. (2026-06-10)
    let body_snap = node.body.clone().unwrap_or_default();
    let latest_body = match state.node_versions.list(node.id, 1).await {
        Ok(versions) => versions.into_iter().next().map(|v| v.body),
        Err(e) => {
            tracing::warn!("latest version lookup failed (non-fatal): {e}");
            None
        }
    };
    if version_snapshot_needed(latest_body.as_deref(), &body_snap)
        && let Err(e) = state
            .node_versions
            .record(node.id, &body_snap, &claims.sub)
            .await
    {
        tracing::warn!("node version snapshot failed (non-fatal): {e}");
    }
    // A status-only save (the status chip) is already in History as
    // `status_changed`; an "Edited" line beside it would say it twice (#208).
    // Otherwise coalesce rapid consecutive edits by the same user into one
    // entry so autosave doesn't flood the dashboard recap. The window is
    // measured from the last *Edited* entry, so a status change in between
    // doesn't split an editing session.
    let log_edit = !status_only
        && match state
            .activity
            .latest_of(node.id, ActivityAction::Edited)
            .await
        {
            Ok(latest) => edit_activity_needed(latest.as_ref(), &claims.sub, chrono::Utc::now()),
            Err(e) => {
                tracing::warn!("latest activity lookup failed (non-fatal): {e}");
                true
            }
        };
    if log_edit {
        log_activity(
            &state,
            node.id,
            &claims,
            ActivityAction::Edited,
            json!({ "title": node.title }),
        )
        .await;
    }
    dispatch(
        state.webhooks.clone(),
        EVENT_NODE_UPDATED,
        Some(node.id),
        &claims.sub,
        &node.owner_id,
    );
    Ok(Json(node))
}

async fn delete_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_owner(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    // Capture the owner before deletion: the webhook fan-out needs it to
    // scope delivery to their endpoints.
    //
    // No activity entry (#208): the log is per node and cascades with it,
    // so a "deleted" row written after the delete always failed its FK and
    // only produced a warning.
    let existing = state.nodes.get(NodeId(id)).await.ok();
    state.nodes.delete(NodeId(id)).await?;
    if let Some(owner_id) = existing.map(|n| n.owner_id) {
        dispatch(
            state.webhooks.clone(),
            EVENT_NODE_DELETED,
            Some(NodeId(id)),
            &claims.sub,
            &owner_id,
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn neighbors(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Node>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let nodes = state
        .nodes
        .neighbors(NodeId(id), subject_scope(&claims))
        .await?;
    Ok(Json(nodes))
}

async fn backlinks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Node>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let nodes = state
        .nodes
        .backlinks(NodeId(id), subject_scope(&claims))
        .await?;
    Ok(Json(nodes))
}

// ── Phase 4: Edges & Tags ────────────────────────────────────────────────────

async fn list_edges_for_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<EdgeWithTitles>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let edges = state
        .edges
        .list_for_node_with_titles(NodeId(id), subject_scope(&claims))
        .await?;
    Ok(Json(edges))
}

/// GET /nodes/{id}/home — the node's one home (#228, §6.1).
///
/// A home the caller can't see is never named (§14). Its editors are told
/// one exists, with its link id and nothing else, so they can free the
/// slot (owner decision): a new home would be refused anyway. A viewer
/// isn't told.
async fn get_home(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<NodeHome>, ApiError> {
    let child = NodeId(id);
    require_viewer(state.permissions.as_ref(), &claims, child).await?;
    Ok(Json(home_for(&state, &claims, child).await?))
}

/// What the caller may know of `child`'s home (see [`get_home`]).
async fn home_for(
    state: &AppState,
    claims: &AuthClaims,
    child: NodeId,
) -> Result<NodeHome, ApiError> {
    let Some(edge) = state.edges.home_of(child).await? else {
        return Ok(NodeHome { home: None });
    };
    let may = |r: Result<(), ApiError>| match r {
        Ok(()) => Ok(true),
        Err(ApiError::Forbidden(_)) => Ok(false),
        Err(e) => Err(e),
    };
    if may(require_viewer(state.permissions.as_ref(), claims, edge.source_id).await)? {
        let parent = state.nodes.get(edge.source_id).await?;
        return Ok(NodeHome {
            home: Some(Home {
                edge_id: edge.id,
                node: Some(HomeNode {
                    id: parent.id,
                    title: parent.title,
                    node_type: parent.node_type,
                }),
            }),
        });
    }
    let editor = may(require_editor(state.permissions.as_ref(), claims, child).await)?;
    Ok(NodeHome {
        home: editor.then_some(Home {
            edge_id: edge.id,
            node: None,
        }),
    })
}

/// PUT /nodes/{id}/home — set the node's one home, or let it stand alone
/// (#228). A home claims the child, so it needs edit rights on both nodes
/// (§14); freeing one needs only the child's. Rights first, then the
/// kinds; the repo checks for a loop under the `contains` lock.
async fn set_home(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<SetHomeRequest>,
) -> Result<Json<NodeHome>, ApiError> {
    let child = NodeId(id);
    require_editor(state.permissions.as_ref(), &claims, child).await?;
    if let Some(parent) = req.parent_id {
        require_editor(state.permissions.as_ref(), &claims, parent).await?;
        if parent == child {
            return Err(ApiError::Validation(
                "a node can't be its own home".to_string(),
            ));
        }
        let parent_node = state.nodes.get(parent).await?;
        let child_node = state.nodes.get(child).await?;
        if let Some(refusal) = edge_kind_violation(
            &EdgeType::Contains,
            &parent_node.node_type,
            &child_node.node_type,
        ) {
            return Err(ApiError::Validation(refusal.message()));
        }
    }
    state.edges.set_home(child, req.parent_id).await?;
    Ok(Json(home_for(&state, &claims, child).await?))
}

async fn list_tags_for_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Tag>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let tags = state.tags.list_for_node(NodeId(id)).await?;
    Ok(Json(tags))
}

async fn attach_tag(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    state.tags.attach(NodeId(id), TagId(tag_id)).await?;
    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::TagAdded,
        json!({ "tag_id": tag_id }),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_tag(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    state.tags.detach(NodeId(id), TagId(tag_id)).await?;
    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::TagRemoved,
        json!({ "tag_id": tag_id }),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// ── Phase 6: Attachments ─────────────────────────────────────────────────────

async fn list_attachments(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Attachment>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let attachments = state.attachments.list(NodeId(id)).await?;
    Ok(Json(attachments))
}

async fn upload_attachment(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Attachment>), ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;

    // Read the first field — expected to be the file.
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::Validation(format!("multipart error: {e}")))?
        .ok_or_else(|| ApiError::Validation("no file field in request".to_string()))?;

    let filename = sanitize_filename(field.file_name().unwrap_or("upload"));
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();

    // SECURITY: allowlist the accepted content types (a blocklist is
    // bypassable — any type not explicitly blocked would slip through). Active
    // types (text/html, image/svg+xml, *javascript) are simply not on the list.
    // SVG is an image type that can carry script, so it is excluded from the
    // `image/` prefix. Defense-in-depth alongside the download path's
    // `Content-Disposition: attachment` + `X-Content-Type-Options: nosniff`.
    const ALLOWED_PREFIXES: &[&str] = &["image/", "audio/", "video/"];
    const ALLOWED_EXACT: &[&str] = &[
        "application/pdf",
        "application/json",
        "application/zip",
        "application/octet-stream",
        "application/msword",
        "application/vnd.ms-excel",
        "application/vnd.ms-powerpoint",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/vnd.oasis.opendocument.text",
        "application/vnd.oasis.opendocument.spreadsheet",
        "application/vnd.oasis.opendocument.presentation",
        "text/plain",
        "text/markdown",
        "text/csv",
        "text/calendar",
    ];
    let ct_lower = content_type.to_lowercase();
    let base = ct_lower.split(';').next().unwrap_or("").trim();
    let allowed = (ALLOWED_PREFIXES.iter().any(|p| base.starts_with(p))
        && !base.starts_with("image/svg"))
        || ALLOWED_EXACT.contains(&base);
    if !allowed {
        return Err(ApiError::Validation(format!(
            "file type '{content_type}' is not allowed"
        )));
    }

    let data: Bytes = field
        .bytes()
        .await
        .map_err(|e| ApiError::Validation(format!("read multipart field: {e}")))?;
    let size_bytes = data.len() as i64;

    // Generate a unique S3 key: <node_id>/<uuid>/<filename>
    let s3_key = format!("{}/{}/{}", id, Uuid::new_v4(), filename);

    state
        .object_store
        .put(&s3_key, data, &content_type)
        .await
        .map_err(|e| ApiError::Storage(e.to_string()))?;

    let attachment = state
        .attachments
        .create(NodeId(id), &filename, &content_type, size_bytes, &s3_key)
        .await?;

    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::AttachmentUploaded,
        json!({ "filename": filename, "content_type": content_type }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(attachment)))
}

// ── Phase 7: Permissions ─────────────────────────────────────────────────────

async fn list_permissions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Permission>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let perms = state.permissions.list(NodeId(id)).await?;
    Ok(Json(perms))
}

async fn grant_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<GrantPermissionRequest>,
) -> Result<(StatusCode, Json<Permission>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    require_owner(state.permissions.as_ref(), &claims, NodeId(id)).await?;

    // SECURITY: a granted Owner must not downgrade the canonical creator.
    let node = state.nodes.get(NodeId(id)).await?;
    guard_canonical_owner(&claims, &node.owner_id, &req.subject_id, Some(&req.role))?;

    let perm = state
        .permissions
        .grant(NodeId(id), &claims.sub, req)
        .await?;
    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::PermissionGranted,
        json!({ "subject_id": perm.subject_id, "role": perm.role }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(perm)))
}

async fn revoke_permission(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((id, perm_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_owner(state.permissions.as_ref(), &claims, NodeId(id)).await?;

    // SECURITY: the permission must belong to THIS node (the route authorized
    // the caller against it — an id from another node is a 404, not a revoke;
    // the IDOR fixed for share tokens in v2.22.4), and a granted Owner must
    // not revoke the canonical creator.
    let node = state.nodes.get(NodeId(id)).await?;
    let perms = state.permissions.list(NodeId(id)).await?;
    let target = perms
        .iter()
        .find(|p| p.id == PermissionId(perm_id))
        .ok_or_else(|| ApiError::NotFound(format!("permission {perm_id} not found")))?;
    guard_canonical_owner(&claims, &node.owner_id, &target.subject_id, None)?;

    state
        .permissions
        .revoke(PermissionId(perm_id), NodeId(id))
        .await?;
    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::PermissionRevoked,
        json!({ "perm_id": perm_id }),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /nodes/{id}/invite`
///
/// Invite a user to collaborate on a node by email.  If the email is already
/// registered in Cognito the permission is granted directly.  If not, a new
/// Cognito user is created (Cognito sends the welcome / temp-password email)
/// and the permission is then granted.
///
/// Requires `Owner` role on the node.  Returns `503` when the Cognito admin
/// client is not configured (i.e. `COGNITO_USER_POOL_ID` is not set).
async fn invite(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<InviteRequest>,
) -> Result<(StatusCode, Json<Permission>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    require_owner(state.permissions.as_ref(), &claims, NodeId(id)).await?;

    let cognito = state.cognito_admin.as_ref().ok_or_else(|| {
        ApiError::Internal("invite requires Cognito admin to be configured".to_string())
    })?;

    // Fetch the node title for the notification email (best-effort; fall back gracefully).
    let node_title = state
        .nodes
        .get(NodeId(id))
        .await
        .map(|n| n.title)
        .unwrap_or_else(|_| "a node".to_string());

    // Look up the email in Cognito.  If not found, create the user (Cognito
    // then sends a welcome / temp-password email automatically).
    // Track whether this is an existing user — we only send an explicit invite
    // notification for existing users; new users get the Cognito welcome email.
    let (subject_id, notify_email) = match cognito.find_user_by_email(&req.email).await? {
        Some(user) => (user.id, Some(req.email.clone())),
        None => {
            let new_user = cognito
                .create_user(&common::admin::CreateAdminUserRequest {
                    email: req.email.clone(),
                    first_name: String::new(),
                    last_name: String::new(),
                    initial_roles: vec!["user".to_string()],
                    send_welcome_email: true,
                })
                .await?;
            // New users receive the Cognito welcome email but no separate
            // invite notification (to avoid sending two emails at once).
            (new_user.id, None)
        }
    };

    let role_str = match req.role {
        PermissionRole::Owner => "owner",
        PermissionRole::Editor => "editor",
        PermissionRole::Viewer => "viewer",
    };

    let grant_req = GrantPermissionRequest {
        subject_id,
        role: req.role,
    };
    let perm = state
        .permissions
        .grant(NodeId(id), &claims.sub, grant_req)
        .await?;

    // Send invite notification to existing users (non-fatal — permission already granted).
    if let Some(email) = notify_email {
        let inviter = claims
            .name
            .as_deref()
            .or(claims.email.as_deref())
            .unwrap_or("A collaborator")
            .to_string();
        let node_id_str = id.to_string();
        maybe_notify_invite(
            state.notifier.as_ref().map(|a| a.as_ref()),
            &email,
            &inviter,
            &node_title,
            role_str,
            &node_id_str,
        )
        .await;
    }

    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::PermissionGranted,
        json!({ "invited_email": req.email, "role": role_str }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(perm)))
}

// ── Activity log ─────────────────────────────────────────────────────────────

/// Fire-and-forget activity record. Failures are logged as warnings, never
/// propagated — the main operation has already succeeded at this point.
pub(crate) async fn log_activity(
    state: &AppState,
    node_id: NodeId,
    claims: &AuthClaims,
    action: ActivityAction,
    extra: serde_json::Value,
) {
    let actor = ActivityActor::from_claims(claims);
    if let Err(e) = state
        .activity
        .record(
            node_id,
            &actor.subject_id,
            action,
            actor.metadata_with(extra),
        )
        .await
    {
        tracing::warn!("activity log write failed (non-fatal): {e}");
    }
}

/// `GET /nodes/{id}/activity?limit=<n>`
///
/// Returns the most recent activity entries for a node.
/// Defaults to the last 50. Requires Viewer permission.
#[derive(Debug, serde::Deserialize)]
struct ActivityParams {
    limit: Option<i64>,
}

async fn list_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Query(params): Query<ActivityParams>,
) -> Result<Json<Vec<ActivityEntry>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let entries = state.activity.list(NodeId(id), limit).await?;
    Ok(Json(entries))
}

// ── Export ────────────────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct ExportParams {
    /// `markdown` (default) or `json`
    format: Option<String>,
}

/// `GET /nodes/{id}/export?format=markdown|json`
///
/// Returns the node as a downloadable file.
/// - `markdown` (default): YAML front-matter + body
/// - `json`: full Node DTO
///
/// Requires at least Viewer permission.
async fn export_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Query(params): Query<ExportParams>,
) -> Result<impl IntoResponse, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let node = state.nodes.get(NodeId(id)).await?;

    let format = params.format.as_deref().unwrap_or("markdown");
    let (body, content_type, filename) = match format {
        "json" => {
            let json = serde_json::to_string_pretty(&node)
                .map_err(|e| ApiError::Internal(format!("JSON serialise failed: {e}")))?;
            let name = export_filename(&node.slug, node.updated_at, "json");
            (json, "application/json; charset=utf-8", name)
        }
        _ => {
            // Build YAML-style front-matter block then append body.
            let node_type = node.node_type.as_str().to_string();
            let status = node.status.as_str().to_string();
            let tag_names: Vec<String> = node.tags.iter().map(|t| t.name.clone()).collect();
            let tags_yaml = if tag_names.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{}]",
                    tag_names
                        .iter()
                        .map(|t| format!("\"{t}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let front_matter = format!(
                "---\ntitle: \"{}\"\ntype: {node_type}\nstatus: {status}\ntags: {tags_yaml}\ncreated_at: {}\nupdated_at: {}\n---\n\n",
                node.title.replace('"', "\\\""),
                node.created_at.format("%Y-%m-%dT%H:%M:%SZ"),
                node.updated_at.format("%Y-%m-%dT%H:%M:%SZ"),
            );
            let md = format!("{}{}", front_matter, node.body.as_deref().unwrap_or(""));
            let name = export_filename(&node.slug, node.updated_at, "md");
            (md, "text/markdown; charset=utf-8", name)
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .map_err(|e| ApiError::Internal(format!("header value: {e}")))?,
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|e| ApiError::Internal(format!("header value: {e}")))?,
    );

    log_activity(
        &state,
        NodeId(id),
        &claims,
        ActivityAction::Exported,
        json!({ "format": format }),
    )
    .await;
    Ok((headers, body))
}

// ── Node versions ─────────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct VersionParams {
    limit: Option<i64>,
}

/// `GET /nodes/{id}/versions?limit=N`
/// Returns up to `limit` (default 20, max 50) most-recent body snapshots.
/// Requires Viewer permission.
async fn list_versions(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Query(params): Query<VersionParams>,
) -> Result<Json<Vec<NodeVersion>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let limit = params.limit.unwrap_or(20).clamp(1, 50);
    let versions = state.node_versions.list(NodeId(id), limit).await?;
    Ok(Json(versions))
}

/// `POST /nodes/{id}/versions/{version_id}/restore`
/// Restores the node body to the selected snapshot. Records a new snapshot
/// of the restored body and logs an Edited activity entry.
/// Requires Editor permission.
async fn restore_version(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((id, version_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Node>, ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let version = state.node_versions.get(NodeVersionId(version_id)).await?;
    // Verify the version belongs to this node.
    if version.node_id != NodeId(id) {
        return Err(ApiError::NotFound(
            "version does not belong to this node".to_string(),
        ));
    }
    let req = UpdateNodeRequest {
        title: None,
        body: Some(version.body.clone()),
        metadata: None,
        status: None,
        project_status: None,
        expected_updated_at: None,
    };
    let node = state
        .nodes
        .update(NodeId(id), req, &ActivityActor::from_claims(&claims))
        .await?;
    sync_wikilinks(&state, node.id, &node.owner_id, &version.body).await?;
    // Snapshot the restored body.
    let sub = claims.sub.clone();
    let ver_repo = state.node_versions.clone();
    let nid = node.id;
    let body_snap = version.body.clone();
    tokio::spawn(async move {
        if let Err(e) = ver_repo.record(nid, &body_snap, &sub).await {
            tracing::warn!("restore version snapshot failed (non-fatal): {e}");
        }
    });
    log_activity(
        &state,
        node.id,
        &claims,
        ActivityAction::Edited,
        json!({ "title": node.title, "restored_from": version_id }),
    )
    .await;
    Ok(Json(node))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve wiki-link titles in `body` to node IDs and sync `wiki_link` edges.
/// SECURITY: titles resolve only among `owner_id`'s nodes (the source node's
/// owner), so a link can never point into another user's private node.
async fn sync_wikilinks(
    state: &AppState,
    source_id: NodeId,
    owner_id: &str,
    body: &str,
) -> Result<(), ApiError> {
    let titles = parse_wikilink_titles(body);
    // Resolve all wiki-link titles in a single batched query rather than one
    // round-trip per title on every node save.
    let target_ids: Vec<NodeId> = if titles.is_empty() {
        Vec::new()
    } else {
        let lowered: Vec<String> = titles.iter().map(|t| t.to_lowercase()).collect();
        let by_title = state.nodes.find_ids_by_titles(owner_id, &lowered).await?;
        lowered
            .iter()
            .filter_map(|lt| by_title.get(lt).copied())
            .collect()
    };
    state.edges.sync_wikilinks(source_id, &target_ids).await?;
    Ok(())
}

/// Strip characters that could inject HTTP headers or break Content-Disposition.
/// Keeps ASCII letters, digits, dots, dashes, underscores, and spaces.
/// Truncates to 200 chars and falls back to "upload" if the result is empty.
/// Duplicate a node — creates a copy with the same body, type, and tags.
/// The new node's title is suffixed with " (copy)" and its status is reset
/// to `Draft`.  The caller becomes the owner of the duplicate.
async fn duplicate_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Node>), ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let src = state.nodes.get(NodeId(id)).await?;

    let req = CreateNodeRequest {
        title: format!("{} (copy)", src.title),
        body: src.body.clone(),
        node_type: src.node_type.clone(),
        status: Some(common::node::NodeStatus::Draft),
        metadata: src.metadata.clone(),
        project_status: copied_status(&src),
        template_id: None,
    };
    let dup = state.nodes.create(&claims.sub, req).await?;

    // Auto-grant Owner permission for the duplicating user.
    let owner_req = GrantPermissionRequest {
        subject_id: claims.sub.clone(),
        role: PermissionRole::Owner,
    };
    state
        .permissions
        .grant(dup.id, &claims.sub, owner_req)
        .await
        .map_err(|e| ApiError::Internal(format!("auto-grant owner permission failed: {e}")))?;

    // Copy tags from the source node.
    let tags = state
        .tags
        .list_for_node(NodeId(id))
        .await
        .unwrap_or_default();
    for tag in tags {
        let _ = state.tags.attach(dup.id, tag.id).await;
    }

    if let Some(ref body) = dup.body {
        sync_wikilinks(&state, dup.id, &dup.owner_id, body).await?;
    }

    let mut meta = json!({ "title": dup.title, "duplicated_from": id });
    with_initial_status(&mut meta, dup.project_status.as_deref());
    log_activity(&state, dup.id, &claims, ActivityAction::Created, meta).await;

    Ok((StatusCode::CREATED, Json(dup)))
}

/// Download name for a single-node export: `{slug}-{updated_at}.{ext}`,
/// e.g. `project-plan-20261023T1937.md`. The slug (not the title) matches the
/// bulk ZIP's per-entry names and is already URL-safe; the `updated_at` stamp
/// keeps successive exports of an edited node from colliding as
/// `Project (1).md`, `Project (2).md`, … and says which version each file is.
fn export_filename(slug: &str, updated_at: chrono::DateTime<chrono::Utc>, ext: &str) -> String {
    let stem: String = sanitize_filename(slug).chars().take(120).collect();
    let stem = if stem == "upload" {
        "node".to_string()
    } else {
        stem
    };
    format!("{stem}-{}.{ext}", updated_at.format("%Y%m%dT%H%M"))
}

fn sanitize_filename(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))
        .take(200)
        .collect();
    if clean.trim().is_empty() {
        "upload".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, Utc};
    use common::id::ActivityId;
    use common::node::NodeType;

    fn create(node_type: NodeType, body: &str, given: Option<&str>) -> CreateNodeRequest {
        CreateNodeRequest {
            title: "t".into(),
            node_type,
            body: Some(body.into()),
            metadata: json!({}),
            status: None,
            project_status: given.map(str::to_string),
            template_id: None,
        }
    }

    #[test]
    fn a_new_project_takes_its_templates_status_token_and_drops_the_copy() {
        let mut req = create(
            NodeType::Project,
            "## Status\n\n- On hold\n\n## Goals\n",
            None,
        );
        initial_project_status(&mut req).unwrap();
        assert_eq!(req.project_status.as_deref(), Some("On hold"));
        assert_eq!(req.body.as_deref(), Some("## Goals\n"));
    }

    #[test]
    fn a_new_project_without_a_token_starts_active() {
        let mut req = create(NodeType::Project, "## Goals\n", None);
        initial_project_status(&mut req).unwrap();
        assert_eq!(req.project_status.as_deref(), Some(project_status::DEFAULT));
        assert_eq!(req.body.as_deref(), Some("## Goals\n"));
    }

    #[test]
    fn a_given_status_wins_is_normalized_and_leaves_the_body_alone() {
        let body = "## Status\n\n- Active\n";
        let mut req = create(NodeType::Project, body, Some("  Blocked  "));
        initial_project_status(&mut req).unwrap();
        assert_eq!(req.project_status.as_deref(), Some("Blocked"));
        assert_eq!(req.body.as_deref(), Some(body));
        let mut bad = create(NodeType::Project, "", Some(&"x".repeat(41)));
        assert!(initial_project_status(&mut bad).is_err());
    }

    #[test]
    fn other_types_get_no_status_and_keep_their_body() {
        let body = "## Status\n\nActive\n";
        let mut req = create(NodeType::Article, body, None);
        initial_project_status(&mut req).unwrap();
        assert_eq!(req.project_status, None);
        assert_eq!(req.body.as_deref(), Some(body));
    }

    #[test]
    fn a_duplicate_keeps_a_projects_status_but_drops_anyone_elses() {
        let node = |node_type: NodeType| Node {
            id: common::id::NodeId(uuid::Uuid::nil()),
            owner_id: "o".into(),
            node_type,
            title: "t".into(),
            slug: "t".into(),
            body: None,
            metadata: json!({}),
            status: common::node::NodeStatus::Draft,
            project_status: Some("Active".into()),
            tags: vec![],
            pinned: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            edge_count: 0,
        };
        assert_eq!(
            copied_status(&node(NodeType::Project)).as_deref(),
            Some("Active")
        );
        assert_eq!(copied_status(&node(NodeType::Article)), None);
    }

    #[test]
    fn a_given_status_on_another_type_is_refused_but_blank_passes() {
        // §3.2 (#216): only Projects have a project status.
        let mut req = create(NodeType::Article, "", Some("Active"));
        let refused = initial_project_status(&mut req);
        assert!(
            matches!(&refused, Err(ApiError::Validation(m)) if m == "only Projects have a project status"),
            "got {refused:?}"
        );
        let mut blank = create(NodeType::Area, "", Some("   "));
        initial_project_status(&mut blank).unwrap();
        assert_eq!(blank.project_status, None, "blank is no status");
    }

    fn entry(action: ActivityAction, subject: &str, age_minutes: i64) -> ActivityEntry {
        ActivityEntry {
            id: ActivityId(Uuid::new_v4()),
            node_id: NodeId(Uuid::new_v4()),
            subject_id: subject.to_string(),
            action,
            metadata: json!({}),
            created_at: Utc::now() - TimeDelta::minutes(age_minutes),
        }
    }

    #[test]
    fn export_filename_uses_slug_and_updated_stamp() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-23T19:37:12Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            export_filename("project-plan", at, "md"),
            "project-plan-20261023T1937.md"
        );
        assert_eq!(
            export_filename("project-plan", at, "json"),
            "project-plan-20261023T1937.json"
        );
    }

    #[test]
    fn export_filename_strips_header_breaking_chars_and_falls_back() {
        let at = Utc::now();
        // Defense in depth: slugs are already URL-safe, but nothing that can
        // break Content-Disposition (quotes, CR/LF) may reach the header.
        let name = export_filename("a\"b\r\nc", at, "md");
        assert!(name.starts_with("abc-"), "{name}");
        assert!(!name.contains(['"', '\r', '\n']));
        assert!(export_filename("", at, "md").starts_with("node-"));
    }

    #[test]
    fn snapshot_needed_when_no_prior_version() {
        assert!(version_snapshot_needed(None, "body"));
    }

    #[test]
    fn snapshot_skipped_when_body_unchanged() {
        // Title/status-only saves and autosave repeats must not pile up
        // duplicate version rows.
        assert!(!version_snapshot_needed(Some("body"), "body"));
    }

    #[test]
    fn snapshot_needed_when_body_changed() {
        assert!(version_snapshot_needed(Some("old"), "new"));
    }

    fn upd() -> UpdateNodeRequest {
        UpdateNodeRequest {
            title: None,
            body: None,
            metadata: None,
            status: None,
            project_status: None,
            expected_updated_at: None,
        }
    }

    #[test]
    fn only_a_bare_status_change_is_status_only() {
        // #208: the status chip's save is logged as status_changed alone.
        let chip = UpdateNodeRequest {
            project_status: Some(Some("Blocked".into())),
            ..upd()
        };
        assert!(is_status_only(&chip));
        let clear = UpdateNodeRequest {
            project_status: Some(None),
            ..upd()
        };
        assert!(is_status_only(&clear), "clearing is a status change too");
        let precondition = UpdateNodeRequest {
            expected_updated_at: Some(chrono::Utc::now()),
            ..chip.clone()
        };
        assert!(
            is_status_only(&precondition),
            "a version check isn't an edit"
        );
        let with_body = UpdateNodeRequest {
            body: Some("x".into()),
            ..chip.clone()
        };
        assert!(!is_status_only(&with_body), "an edit still logs Edited");
        assert!(!is_status_only(&upd()), "no status, not status-only");
    }

    #[test]
    fn a_created_entry_records_the_initial_status_when_there_is_one() {
        let mut meta = json!({ "title": "p" });
        with_initial_status(&mut meta, Some("Active"));
        assert_eq!(meta, json!({ "title": "p", "project_status": "Active" }));
        let mut none = json!({ "title": "a" });
        with_initial_status(&mut none, None);
        assert_eq!(none, json!({ "title": "a" }));
    }

    #[test]
    fn edit_log_needed_when_no_prior_activity() {
        assert!(edit_activity_needed(None, "sub-a", Utc::now()));
    }

    #[test]
    fn edit_log_coalesced_within_window_same_subject() {
        let e = entry(ActivityAction::Edited, "sub-a", 5);
        assert!(!edit_activity_needed(Some(&e), "sub-a", Utc::now()));
    }

    #[test]
    fn edit_log_needed_after_window_elapsed() {
        let e = entry(ActivityAction::Edited, "sub-a", 20);
        assert!(edit_activity_needed(Some(&e), "sub-a", Utc::now()));
    }

    #[test]
    fn edit_log_needed_for_different_subject() {
        let e = entry(ActivityAction::Edited, "sub-b", 5);
        assert!(edit_activity_needed(Some(&e), "sub-a", Utc::now()));
    }
}
