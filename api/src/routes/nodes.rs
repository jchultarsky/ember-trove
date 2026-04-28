use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use bytes::Bytes;
use common::{
    activity::{ActivityAction, ActivityEntry},
    attachment::Attachment,
    auth::AuthClaims,
    edge::EdgeWithTitles,
    id::{NodeId, NodeVersionId, PermissionId, TagId},
    node::{CreateNodeRequest, Node, NodeListParams, NodeListResponse, NodeTitleEntry, UpdateNodeRequest},
    node_version::NodeVersion,
    permission::{GrantPermissionRequest, InviteRequest, Permission, PermissionRole},
    tag::Tag,
};
use garde::Validate;
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::permissions::{is_admin, require_editor, require_owner, require_viewer},
    error::ApiError,
    notify::maybe_notify_invite,
    state::AppState,
    wikilink::parse_wikilink_titles,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_nodes).post(create_node))
        .route("/titles", get(list_titles))
        .route("/slug/{slug}", get(get_node_by_slug))
        .route("/{id}", get(get_node).put(update_node).delete(delete_node))
        .route("/{id}/neighbors", get(neighbors))
        .route("/{id}/backlinks", get(backlinks))
        .route("/{id}/edges", get(list_edges_for_node))
        .route("/{id}/tags", get(list_tags_for_node))
        .route("/{id}/tags/{tag_id}", post(attach_tag).delete(detach_tag))
        .route(
            "/{id}/attachments",
            get(list_attachments).post(upload_attachment)
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
    let has_more = ((page as u64) * (per_page as u64)) + (nodes.len() as u64) < total;

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

// ── Node CRUD ────────────────────────────────────────────────────────────────

async fn create_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateNodeRequest>,
) -> Result<(StatusCode, Json<Node>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

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

    sync_wikilinks(&state, node.id, node.body.as_deref().unwrap_or("")).await?;
    let (action, meta) = if let Some(tid) = template_id {
        (ActivityAction::CreatedFromTemplate, json!({ "title": node.title, "template_id": tid }))
    } else {
        (ActivityAction::Created, json!({ "title": node.title }))
    };
    log_activity(&state, node.id, &claims, action, meta).await;
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

async fn update_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateNodeRequest>,
) -> Result<Json<Node>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let node = state.nodes.update(NodeId(id), req).await?;
    sync_wikilinks(&state, node.id, node.body.as_deref().unwrap_or("")).await?;
    // Record body snapshot (fire-and-forget — failure is non-fatal).
    let body_snap = node.body.clone().unwrap_or_default();
    let sub = claims.sub.clone();
    let ver_repo = state.node_versions.clone();
    let nid = node.id;
    tokio::spawn(async move {
        if let Err(e) = ver_repo.record(nid, &body_snap, &sub).await {
            tracing::warn!("node version snapshot failed (non-fatal): {e}");
        }
    });
    log_activity(&state, node.id, &claims, ActivityAction::Edited, json!({ "title": node.title })).await;
    Ok(Json(node))
}

async fn delete_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_owner(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    // Capture the title before deletion for the activity entry (cascade will remove it after).
    let title = state.nodes.get(NodeId(id)).await.map(|n| n.title).unwrap_or_default();
    state.nodes.delete(NodeId(id)).await?;
    log_activity(&state, NodeId(id), &claims, ActivityAction::Deleted, json!({ "title": title })).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn neighbors(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Node>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let nodes = state.nodes.neighbors(NodeId(id)).await?;
    Ok(Json(nodes))
}

async fn backlinks(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Node>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let nodes = state.nodes.backlinks(NodeId(id)).await?;
    Ok(Json(nodes))
}

// ── Phase 4: Edges & Tags ────────────────────────────────────────────────────

async fn list_edges_for_node(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<EdgeWithTitles>>, ApiError> {
    require_viewer(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    let edges = state.edges.list_for_node_with_titles(NodeId(id)).await?;
    Ok(Json(edges))
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
    log_activity(&state, NodeId(id), &claims, ActivityAction::TagAdded, json!({ "tag_id": tag_id })).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_tag(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path((id, tag_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_editor(state.permissions.as_ref(), &claims, NodeId(id)).await?;
    state.tags.detach(NodeId(id), TagId(tag_id)).await?;
    log_activity(&state, NodeId(id), &claims, ActivityAction::TagRemoved, json!({ "tag_id": tag_id })).await;
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

    // SECURITY: Reject content types that could enable stored XSS or malware.
    const BLOCKED_TYPES: &[&str] = &[
        "text/html",
        "application/xhtml+xml",
        "image/svg+xml",
        "application/javascript",
        "text/javascript",
        "application/x-executable",
        "application/x-sharedlib",
        "application/x-msdos-program",
    ];
    let ct_lower = content_type.to_lowercase();
    if BLOCKED_TYPES.iter().any(|b| ct_lower.starts_with(b)) {
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
    state.permissions.revoke(PermissionId(perm_id)).await?;
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
    let mut meta = json!({
        "actor_name": claims.name,
        "actor_email": claims.email,
    });
    // Merge in caller-supplied extra fields.
    if let (Some(obj), Some(ext)) = (meta.as_object_mut(), extra.as_object()) {
        for (k, v) in ext {
            obj.insert(k.clone(), v.clone());
        }
    }
    if let Err(e) = state.activity.record(node_id, &claims.sub, action, meta).await {
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
            let name = sanitize_filename(&format!("{}.json", node.title));
            (json, "application/json; charset=utf-8", name)
        }
        _ => {
            // Build YAML-style front-matter block then append body.
            let node_type = format!("{:?}", node.node_type).to_lowercase();
            let status = format!("{:?}", node.status).to_lowercase();
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
            let md = format!(
                "{}{}",
                front_matter,
                node.body.as_deref().unwrap_or("")
            );
            let name = sanitize_filename(&format!("{}.md", node.title));
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
        return Err(ApiError::NotFound("version does not belong to this node".to_string()));
    }
    let req = UpdateNodeRequest {
        title: None,
        body: Some(version.body.clone()),
        metadata: None,
        status: None,
    };
    let node = state.nodes.update(NodeId(id), req).await?;
    sync_wikilinks(&state, node.id, &version.body).await?;
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
    log_activity(&state, node.id, &claims, ActivityAction::Edited, json!({ "title": node.title, "restored_from": version_id })).await;
    Ok(Json(node))
}


// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve wiki-link titles in `body` to node IDs and sync `wiki_link` edges.
async fn sync_wikilinks(
    state: &AppState,
    source_id: NodeId,
    body: &str,
) -> Result<(), ApiError> {
    let titles = parse_wikilink_titles(body);
    let mut target_ids = Vec::new();
    for title in &titles {
        if let Some(id) = state.nodes.find_id_by_title(title).await? {
            target_ids.push(id);
        }
    }
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
    let tags = state.tags.list_for_node(NodeId(id)).await.unwrap_or_default();
    for tag in tags {
        let _ = state.tags.attach(dup.id, tag.id).await;
    }

    if let Some(ref body) = dup.body {
        sync_wikilinks(&state, dup.id, body).await?;
    }

    log_activity(
        &state,
        dup.id,
        &claims,
        ActivityAction::Created,
        json!({ "title": dup.title, "duplicated_from": id }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(dup)))
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
