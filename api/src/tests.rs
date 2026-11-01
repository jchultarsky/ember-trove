//! Integration tests for the Axum router.
//!
//! These tests build the full production router (minus a live database) and
//! send requests through it via `tower::ServiceExt::oneshot`, verifying HTTP
//! status codes, JSON shape, and route registration.
//!
//! ## Design constraints
//! * No live database — `PgPool::connect_lazy` defers the TCP dial until the
//!   first query.  Routes that hit the DB return a "degraded" payload rather
//!   than the happy path; we only assert HTTP status codes here.
//! * No OIDC — `state.oidc = None`.  The `require_auth` middleware returns
//!   `500 Internal Server Error` ("OIDC not configured") for every protected
//!   route, which is distinct from `404 Not Found`.  We exploit this to
//!   verify that every route is **registered** without setting up real auth.
//! * Handler-level authorization: `claims_app` mounts one router slice with
//!   injected `AuthClaims`, so the real handlers run against the stub repos.
//!   The "Authorization world" fixture (nodes, roles, permission rows, tasks)
//!   backs the authZ tests; a write a handler should have refused panics in
//!   the stub, so a missing check can't pass on status code alone.
//! * Rate limiter — `SmartIpKeyExtractor` needs at least one of
//!   `X-Real-IP`, `X-Forwarded-For`, or `ConnectInfo<SocketAddr>`.
//!   All requests to rate-limited routes include `X-Forwarded-For: 127.0.0.1`.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use axum_extra::extract::cookie::Key;
use chrono::NaiveDate;
use common::{
    EmberTroveError,
    activity::{ActivityAction, ActivityEntry},
    attachment::Attachment,
    backup::BackupJob,
    edge::{CreateEdgeRequest, Edge, EdgeWithTitles},
    favorite::{CreateFavoriteRequest, Favorite},
    graph::NodePosition,
    id::{AttachmentId, EdgeId, FavoriteId, NodeId, PermissionId, TagId, TaskId, TemplateId},
    id::{NodeLinkId, SearchPresetId, WebhookId},
    node::{
        CreateNodeRequest, Node, NodeListParams, NodeStatus, NodeTitleEntry, NodeType,
        UpdateNodeRequest,
    },
    node_link::{CreateNodeLinkRequest, NodeLink, UpdateNodeLinkRequest},
    note::{CreateNoteRequest, FeedNote, Note},
    permission::{GrantPermissionRequest, Permission, PermissionRole},
    search::{CreateSearchPresetRequest, SearchPreset, SearchQuery, SearchResponse},
    tag::{CreateTagRequest, Tag, UpdateTagRequest},
    task::{
        CreateTaskRequest, MyDayTask, Task, TaskCounts, TaskPriority, TaskStatus, UpdateTaskRequest,
    },
    template::{CreateTemplateRequest, NodeTemplate, UpdateTemplateRequest},
    webhook::{CreateWebhookRequest, UpdateWebhookRequest, Webhook},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    auth::AuthConfig,
    config::Config,
    object_store::NullObjectStore,
    repo::{
        activity::ActivityRepo, attachment::AttachmentRepo, backup::BackupRepo, edge::EdgeRepo,
        favorite::FavoriteRepo, graph::GraphRepo, node::NodeRepo, node_link::NodeLinkRepo,
        node_version::NodeVersionRepo, note::NoteRepo, permission::PermissionRepo, pkce::PkceRepo,
        search::SearchRepo, search_presets::SearchPresetRepo, tag::TagRepo, task::TaskRepo,
        template::TemplateRepo, webhook::WebhookRepo,
    },
    routes::build_router,
    state::AppState,
};

// ── Stub repo implementations ─────────────────────────────────────────────────
// Most methods panic with `unimplemented!()`: they are never reached because
// the auth middleware short-circuits every protected request when oidc = None.
// Handler tests that inject claims (`claims_app`) get canned data instead —
// the restore flow, export, and the "Authorization world"
// below that the authZ tests run against.

struct StubNodeRepo;
#[async_trait]
impl NodeRepo for StubNodeRepo {
    async fn create(&self, _: &str, _: CreateNodeRequest) -> Result<Node, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, id: NodeId) -> Result<Node, EmberTroveError> {
        // Canonical owners for the authz world: the creator of AZ_OWNED_NODE is
        // "creator" (test-sub holds a *granted* Owner row there).
        let (owner, node_type) = match id.0.as_u128() {
            AZ_OWNED_NODE => ("creator", NodeType::Article),
            AZ_EDIT_NODE | AZ_VIEW_NODE | AZ_FOREIGN_NODE => ("someone-else", NodeType::Article),
            AZ_MY_PROJECT => ("test-sub", NodeType::Project),
            AZ_MY_ARTICLE => ("test-sub", NodeType::Article),
            _ => return Err(EmberTroveError::NotFound(format!("node {id}"))),
        };
        Ok(Node {
            id,
            node_type,
            ..fake_node("authz-node", owner)
        })
    }
    async fn get_by_slug(&self, _: &str) -> Result<Node, EmberTroveError> {
        unimplemented!()
    }
    async fn list(&self, params: NodeListParams) -> Result<(Vec<Node>, u64), EmberTroveError> {
        // One node owned by whoever was asked for (the project dashboard).
        let owner = params.owner_id.as_deref().unwrap_or("anyone");
        Ok((vec![fake_node("listed", owner)], 1))
    }
    async fn update(
        &self,
        _: NodeId,
        req: UpdateNodeRequest,
        _: &crate::repo::activity::ActivityActor,
    ) -> Result<Node, EmberTroveError> {
        // A precondition always "fails" here, so a 412 proves the handler
        // got past authorization; anything else must not reach the repo.
        if req.expected_updated_at.is_some() {
            return Err(EmberTroveError::PreconditionFailed("stub".into()));
        }
        unimplemented!()
    }
    async fn delete(&self, id: NodeId) -> Result<(), EmberTroveError> {
        // Only the node test-sub owns may be deleted; reaching here for any
        // other node is an authorization bypass.
        match id.0.as_u128() {
            AZ_OWNED_NODE => Ok(()),
            _ => panic!("authorization bypass: node delete reached the repo for {id}"),
        }
    }
    async fn neighbors(&self, _: NodeId, _: Option<&str>) -> Result<Vec<Node>, EmberTroveError> {
        unimplemented!()
    }
    async fn backlinks(&self, _: NodeId, _: Option<&str>) -> Result<Vec<Node>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_titles(&self, _: Option<&str>) -> Result<Vec<NodeTitleEntry>, EmberTroveError> {
        unimplemented!()
    }
    async fn find_ids_by_titles(
        &self,
        owner: &str,
        titles: &[String],
    ) -> Result<std::collections::HashMap<String, NodeId>, EmberTroveError> {
        // SECURITY (#134): resolution must run as the caller, never another
        // subject — a different owner here is a bypass, so fail loudly.
        assert_eq!(
            owner, "test-sub",
            "titles resolved on someone else's behalf"
        );
        Ok(titles
            .iter()
            .filter(|t| t.as_str() != "not visible")
            .map(|t| (t.clone(), NodeId(Uuid::from_u128(t.len() as u128))))
            .collect())
    }
    async fn lookup_titles(
        &self,
        subject: Option<&str>,
        q: &str,
        limit: i64,
        node_type: Option<&NodeType>,
    ) -> Result<Vec<common::node::NodeLookupHit>, EmberTroveError> {
        // Echo the scope, limit and kind back so the handler test can see
        // what it passed (the SQL itself is covered in repo::pg_tests).
        Ok(vec![common::node::NodeLookupHit {
            id: NodeId(Uuid::nil()),
            title: format!("{}|{q}|{limit}", subject.unwrap_or("<admin>")),
            node_type: node_type.cloned().unwrap_or(NodeType::Article),
        }])
    }
    async fn list_all_for_owner(&self, owner: &str) -> Result<Vec<Node>, EmberTroveError> {
        // One node owned by the caller — lets the export test prove
        // owner-scoping against `list_all` (which returns two).
        Ok(vec![fake_node("owned-node", owner)])
    }
    async fn area_for_nodes(
        &self,
        ids: &[NodeId],
        subject: Option<&str>,
    ) -> Result<Vec<(NodeId, NodeId, String)>, EmberTroveError> {
        // Echo the scope so tests can prove the route asks for the caller's.
        Ok(ids
            .iter()
            .map(|id| (*id, *id, format!("area for {subject:?}")))
            .collect())
    }
    async fn project_status_counts(
        &self,
        owner_id: &str,
    ) -> Result<Vec<common::node::ProjectStatusCount>, EmberTroveError> {
        // Echo the owner so tests can prove the route asks for the CALLER's.
        Ok(vec![common::node::ProjectStatusCount {
            status: format!("of {owner_id}"),
            count: 1,
        }])
    }
    async fn set_pinned(&self, _: NodeId, _: bool) -> Result<Node, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<Node>, EmberTroveError> {
        Ok(vec![
            fake_node("owned-node", "test-sub"),
            fake_node("other-users-node", "someone-else"),
        ])
    }
}

struct StubEdgeRepo;
#[async_trait]
impl EdgeRepo for StubEdgeRepo {
    async fn create(&self, req: CreateEdgeRequest) -> Result<Edge, EmberTroveError> {
        // The authz world's AZ_VIEW_NODE is "already linked" (the DB's unique
        // index answers this in production — migration 034, #129).
        if req.target_id == az_node(AZ_VIEW_NODE) {
            return Err(EmberTroveError::AlreadyExists(
                "already linked as references".to_string(),
            ));
        }
        Ok(Edge {
            id: EdgeId(Uuid::new_v4()),
            source_id: req.source_id,
            target_id: req.target_id,
            edge_type: req.edge_type,
            label: req.label,
            created_at: chrono::Utc::now(),
        })
    }
    async fn get(&self, id: EdgeId) -> Result<Edge, EmberTroveError> {
        // Two edges from a node the caller can't touch into their own
        // Project (#224): a home and a plain link.
        let edge_type = match id.0.as_u128() {
            AZ_FOREIGN_HOME_EDGE | AZ_VISIBLE_HOME_EDGE => common::edge::EdgeType::Contains,
            AZ_FOREIGN_LINK_EDGE => common::edge::EdgeType::References,
            AZ_FOREIGN_RELATED_EDGE | AZ_STRANGERS_RELATED_EDGE => {
                common::edge::EdgeType::RelatedTo
            }
            _ => return Err(EmberTroveError::NotFound(format!("edge {id}"))),
        };
        let target = if id.0.as_u128() == AZ_STRANGERS_RELATED_EDGE {
            AZ_VIEW_NODE
        } else {
            AZ_MY_PROJECT
        };
        let source = if id.0.as_u128() == AZ_VISIBLE_HOME_EDGE {
            AZ_VIEW_NODE
        } else {
            AZ_FOREIGN_NODE
        };
        Ok(Edge {
            id,
            source_id: az_node(source),
            target_id: az_node(target),
            edge_type,
            label: None,
            created_at: chrono::Utc::now(),
        })
    }
    async fn delete(&self, _: EdgeId) -> Result<(), EmberTroveError> {
        Ok(())
    }
    async fn convert(
        &self,
        _: EdgeId,
        _: common::edge::EdgeType,
        _: common::edge::EdgeType,
        swap: bool,
    ) -> Result<(), EmberTroveError> {
        // The only allowed conversion in this world runs from the child's
        // side (the caller edits the target only), so the ends must swap.
        assert!(swap, "a related link runs from the end the caller edits");
        Ok(())
    }
    async fn list_for_node(&self, _: NodeId) -> Result<Vec<Edge>, EmberTroveError> {
        unimplemented!()
    }
    async fn home_of(&self, child: NodeId) -> Result<Option<Edge>, EmberTroveError> {
        // See AZ_HIDDEN_HOME_EDGE.
        let (edge, parent) = if child == az_node(AZ_MY_PROJECT) || child == az_node(AZ_VIEW_NODE) {
            (AZ_HIDDEN_HOME_EDGE, AZ_FOREIGN_NODE)
        } else if child == az_node(AZ_EDIT_NODE) {
            (AZ_SEEN_HOME_EDGE, AZ_OWNED_NODE)
        } else {
            return Ok(None);
        };
        Ok(Some(Edge {
            id: EdgeId(Uuid::from_u128(edge)),
            source_id: az_node(parent),
            target_id: child,
            edge_type: common::edge::EdgeType::Contains,
            label: None,
            created_at: chrono::Utc::now(),
        }))
    }
    async fn set_home(
        &self,
        child: NodeId,
        parent: Option<NodeId>,
    ) -> Result<Option<Edge>, EmberTroveError> {
        // Only a call that passed every route check gets here: freeing the
        // caller's own Project. Anything else is a missing check.
        assert_eq!(
            (child, parent),
            (az_node(AZ_MY_PROJECT), None),
            "set_home reached past the route's checks"
        );
        Ok(None)
    }
    async fn list_for_node_with_titles(
        &self,
        _: NodeId,
        _: Option<&str>,
    ) -> Result<Vec<EdgeWithTitles>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<Edge>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_visible(&self, _: Option<&str>) -> Result<Vec<Edge>, EmberTroveError> {
        unimplemented!()
    }
    async fn sync_wikilinks(&self, _: NodeId, _: &[NodeId]) -> Result<(), EmberTroveError> {
        // A converted capture's node links its body (#234); nothing to keep.
        Ok(())
    }
}

struct StubTagRepo;
#[async_trait]
impl TagRepo for StubTagRepo {
    async fn create(&self, _: &str, _: CreateTagRequest) -> Result<Tag, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, _: TagId) -> Result<Tag, EmberTroveError> {
        unimplemented!()
    }
    async fn list(&self, _: &str) -> Result<Vec<Tag>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<Tag>, EmberTroveError> {
        unimplemented!()
    }
    async fn update(&self, _: TagId, _: UpdateTagRequest) -> Result<Tag, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: TagId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn attach(&self, _: NodeId, _: TagId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn detach(&self, _: NodeId, _: TagId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn list_for_node(&self, _: NodeId) -> Result<Vec<Tag>, EmberTroveError> {
        unimplemented!()
    }
}

// ── Authorization world ──────────────────────────────────────────────────────
// A fixed set of nodes, permission rows and tasks for the handler-level
// authorization tests: the caller (`fake_claims`, sub "test-sub") holds a
// granted Owner row on AZ_OWNED_NODE (canonical owner "creator"), Editor on
// AZ_EDIT_NODE, Viewer on AZ_VIEW_NODE and nothing on AZ_FOREIGN_NODE.
// Repo writes the handler must refuse panic (see StubTaskRepo::update,
// StubPermissionRepo::revoke), so a missing check fails the test even if the
// response would otherwise look fine.

const AZ_OWNED_NODE: u128 = 0xa2_0001;
const AZ_EDIT_NODE: u128 = 0xa2_0002;
const AZ_VIEW_NODE: u128 = 0xa2_0003;
const AZ_FOREIGN_NODE: u128 = 0xa2_0004;
/// The caller's own Project and own Article (canonical owner "test-sub"),
/// for the §3.2 capability gates (#216). Every other authz node is an
/// Article (`fake_node`).
const AZ_MY_PROJECT: u128 = 0xa2_0005;
const AZ_MY_ARTICLE: u128 = 0xa2_0006;
/// Edges from AZ_FOREIGN_NODE into AZ_MY_PROJECT (#224).
const AZ_FOREIGN_HOME_EDGE: u128 = 0xa2_0e30;
const AZ_FOREIGN_LINK_EDGE: u128 = 0xa2_0e31;
const AZ_FOREIGN_RELATED_EDGE: u128 = 0xa2_0e32;
/// A related link between two nodes the caller can't edit.
const AZ_STRANGERS_RELATED_EDGE: u128 = 0xa2_0e33;
/// A home from AZ_VIEW_NODE (seen, not edited) into AZ_MY_PROJECT (#228).
const AZ_VISIBLE_HOME_EDGE: u128 = 0xa2_0e34;
/// `home_of` answers (#228): AZ_MY_PROJECT's home is hidden (AZ_FOREIGN_NODE)
/// and the caller edits the child; AZ_VIEW_NODE's is hidden too, but the
/// caller only views the child; AZ_EDIT_NODE's (AZ_OWNED_NODE) is visible.
const AZ_HIDDEN_HOME_EDGE: u128 = 0xa2_0e35;
const AZ_SEEN_HOME_EDGE: u128 = 0xa2_0e36;

/// Someone else's standalone task.
const AZ_FOREIGN_TASK: u128 = 0xa2_0101;
/// Someone else's task on a node the caller can only view.
const AZ_VIEW_TASK: u128 = 0xa2_0102;
/// Someone else's task on a node the caller can edit.
const AZ_EDIT_TASK: u128 = 0xa2_0103;

const AZ_PERM_CREATOR: u128 = 0xa2_0201;
const AZ_PERM_GUEST: u128 = 0xa2_0202;
const AZ_PERM_FOREIGN: u128 = 0xa2_0203;

fn az_node(n: u128) -> NodeId {
    NodeId(Uuid::from_u128(n))
}

fn authz_permissions() -> Vec<Permission> {
    let row = |id: u128, node: u128, subject: &str, role: PermissionRole| Permission {
        id: PermissionId(Uuid::from_u128(id)),
        node_id: az_node(node),
        subject_id: subject.to_string(),
        role,
        granted_by: "creator".to_string(),
        created_at: chrono::Utc::now(),
    };
    vec![
        row(
            AZ_PERM_CREATOR,
            AZ_OWNED_NODE,
            "creator",
            PermissionRole::Owner,
        ),
        row(0xa2_0204, AZ_OWNED_NODE, "test-sub", PermissionRole::Owner),
        row(
            AZ_PERM_GUEST,
            AZ_OWNED_NODE,
            "guest",
            PermissionRole::Viewer,
        ),
        row(0xa2_0205, AZ_EDIT_NODE, "test-sub", PermissionRole::Editor),
        row(0xa2_0207, AZ_MY_PROJECT, "test-sub", PermissionRole::Owner),
        row(0xa2_0208, AZ_MY_ARTICLE, "test-sub", PermissionRole::Owner),
        row(0xa2_0206, AZ_VIEW_NODE, "test-sub", PermissionRole::Viewer),
        row(
            AZ_PERM_FOREIGN,
            AZ_FOREIGN_NODE,
            "guest",
            PermissionRole::Viewer,
        ),
    ]
}

fn authz_task(id: TaskId) -> Task {
    let (owner, node) = match id.0.as_u128() {
        AZ_FOREIGN_TASK => ("someone-else", None),
        AZ_VIEW_TASK => ("someone-else", Some(az_node(AZ_VIEW_NODE))),
        AZ_EDIT_TASK => ("someone-else", Some(az_node(AZ_EDIT_NODE))),
        _ => ("test-sub", None),
    };
    Task {
        owner_id: owner.to_string(),
        node_id: node,
        ..stub_task(id)
    }
}

/// Canned task for the restore-handler tests. Owned by [`fake_claims`]'s sub
/// so the handler's owner check passes.
fn stub_task(id: TaskId) -> Task {
    Task {
        id,
        node_id: None,
        owner_id: "test-sub".to_string(),
        title: "stub".to_string(),
        status: TaskStatus::Open,
        priority: TaskPriority::Medium,
        focus_date: None,
        due_date: None,
        recurrence: None,
        sort_order: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

struct StubTaskRepo;
#[async_trait]
impl TaskRepo for StubTaskRepo {
    async fn create(
        &self,
        node: Option<NodeId>,
        owner: &str,
        req: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError> {
        // Only a standalone task or one on the caller's Project may get
        // here; anything else is a gate the handler skipped (#216).
        match node.map(|n| n.0.as_u128()) {
            None | Some(AZ_MY_PROJECT) => Ok(Task {
                owner_id: owner.to_string(),
                node_id: node,
                title: req.title,
                ..stub_task(TaskId(Uuid::new_v4()))
            }),
            Some(other) => panic!("capability bypass: task created on node {other:#x}"),
        }
    }
    async fn list_for_node(&self, _: NodeId, _: &str) -> Result<Vec<Task>, EmberTroveError> {
        unimplemented!()
    }
    async fn create_occurrence(
        &self,
        _: TaskId,
        _: &str,
        _: CreateTaskRequest,
    ) -> Result<Task, EmberTroveError> {
        unimplemented!()
    }
    async fn retract_occurrence(&self, _: TaskId, _: &str) -> Result<u64, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, id: TaskId) -> Result<Task, EmberTroveError> {
        Ok(authz_task(id))
    }
    async fn update(&self, id: TaskId, req: UpdateTaskRequest) -> Result<Task, EmberTroveError> {
        // A write the handler should have refused fails the test loudly
        // instead of quietly returning 200 (see the authz world below).
        let dest = req.node_id.flatten().map(|n| n.0.as_u128());
        if matches!(id.0.as_u128(), AZ_FOREIGN_TASK | AZ_VIEW_TASK)
            || matches!(dest, Some(AZ_VIEW_NODE | AZ_FOREIGN_NODE))
        {
            panic!("authorization bypass: update reached the repo for task {id}");
        }
        if matches!(dest, Some(AZ_MY_ARTICLE | AZ_OWNED_NODE)) {
            panic!("capability bypass: task {id} moved onto an Article");
        }
        let mut task = authz_task(id);
        if let Some(node) = req.node_id {
            task.node_id = node;
        }
        if let Some(title) = req.title {
            task.title = title;
        }
        Ok(task)
    }
    async fn delete(&self, _: TaskId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    // The restore-flow stubs return canned data (instead of panicking) so the
    // restore-handler tests below can drive the real handler through a bare
    // router slice with injected claims.
    async fn get_deleted(&self, id: TaskId) -> Result<Task, EmberTroveError> {
        Ok(stub_task(id))
    }
    async fn restore(&self, id: TaskId) -> Result<Task, EmberTroveError> {
        Ok(stub_task(id))
    }
    async fn purge_deleted_before(
        &self,
        _: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, EmberTroveError> {
        Ok(0)
    }
    async fn list_my_day(&self, _: &str, _: NaiveDate) -> Result<Vec<MyDayTask>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_inbox(&self, _: &str) -> Result<Vec<Task>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_open_for_owner(&self, _: &str) -> Result<Vec<MyDayTask>, EmberTroveError> {
        unimplemented!()
    }
    async fn counts_for_nodes(
        &self,
        _: &[NodeId],
    ) -> Result<Vec<(NodeId, TaskCounts)>, EmberTroveError> {
        Ok(Vec::new())
    }
    async fn list_open_for_nodes(
        &self,
        _: &[NodeId],
        _: i64,
    ) -> Result<Vec<(NodeId, Vec<common::task::TaskSummary>, bool)>, EmberTroveError> {
        Ok(Vec::new())
    }
    async fn max_task_updated_for_nodes(
        &self,
        _: &[NodeId],
    ) -> Result<Vec<(NodeId, chrono::DateTime<chrono::Utc>)>, EmberTroveError> {
        Ok(Vec::new())
    }
    async fn list_all_for_owner(&self, _: &str) -> Result<Vec<Task>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<Task>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_by_due_range(
        &self,
        _: &str,
        _: NaiveDate,
        _: NaiveDate,
    ) -> Result<Vec<MyDayTask>, EmberTroveError> {
        unimplemented!()
    }
    async fn reorder(&self, _: &[(TaskId, i32)], _: &str) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

/// Captures (#234): one belongs to "someone-else" (`AZ_OTHERS_CAPTURE`);
/// every call names the owner it was asked for, so a test sees whose
/// captures a route reached. Anyone but the owner gets `NotFound`, as the
/// real repo's `WHERE owner_id = $caller` does.
const AZ_OTHERS_CAPTURE: u128 = 0xa2_0c01;
struct StubCaptureRepo;
#[async_trait]
impl crate::repo::capture::CaptureRepo for StubCaptureRepo {
    async fn list(&self, owner: &str) -> Result<Vec<common::capture::Capture>, EmberTroveError> {
        Ok(vec![stub_capture(Uuid::new_v4(), owner, "mine")])
    }
    async fn create(
        &self,
        owner: &str,
        body: &str,
    ) -> Result<common::capture::Capture, EmberTroveError> {
        Ok(stub_capture(Uuid::new_v4(), owner, body))
    }
    async fn update(
        &self,
        id: common::id::CaptureId,
        owner: &str,
        body: &str,
    ) -> Result<common::capture::Capture, EmberTroveError> {
        stub_owned(id, owner)?;
        Ok(stub_capture(id.0, owner, body))
    }
    async fn delete(&self, id: common::id::CaptureId, owner: &str) -> Result<(), EmberTroveError> {
        stub_owned(id, owner)
    }
    async fn restore(
        &self,
        id: common::id::CaptureId,
        owner: &str,
    ) -> Result<common::capture::Capture, EmberTroveError> {
        stub_owned(id, owner)?;
        Ok(stub_capture(id.0, owner, "back"))
    }
    async fn purge_deleted_before(
        &self,
        _: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<common::capture::Capture>, EmberTroveError> {
        unimplemented!()
    }
    async fn convert(
        &self,
        id: common::id::CaptureId,
        owner: &str,
        into: crate::repo::capture::Destination,
    ) -> Result<crate::repo::capture::Made, EmberTroveError> {
        use crate::repo::capture::{Destination, Made};
        stub_owned(id, owner)?;
        // Only destinations the caller may write reach the repo; anything
        // else is a check the handler skipped, so the test fails loudly.
        let editable = |n: NodeId| {
            matches!(
                n.0.as_u128(),
                AZ_OWNED_NODE | AZ_EDIT_NODE | AZ_MY_PROJECT | AZ_MY_ARTICLE
            )
        };
        Ok(match into {
            Destination::Node { req, home } => {
                assert!(home.is_none_or(editable), "home not checked");
                Made::Node(Box::new(Node {
                    node_type: req.node_type,
                    title: req.title,
                    project_status: req.project_status,
                    ..fake_node("converted", owner)
                }))
            }
            Destination::Note { node_id, req } => {
                assert!(editable(node_id), "note's node not checked");
                Made::Note(Note {
                    id: common::id::NoteId(Uuid::new_v4()),
                    node_id,
                    owner_id: owner.to_string(),
                    body: req.body,
                    color: req.color,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                })
            }
            Destination::Task { node_id, req } => {
                assert_eq!(node_id.0.as_u128(), AZ_MY_PROJECT, "task gates skipped");
                Made::Task {
                    project: node_id,
                    task: Task {
                        node_id: Some(node_id),
                        owner_id: owner.to_string(),
                        title: req.title,
                        ..stub_task(TaskId(Uuid::new_v4()))
                    },
                }
            }
        })
    }
}

fn stub_capture(id: Uuid, owner: &str, body: &str) -> common::capture::Capture {
    common::capture::Capture {
        id: common::id::CaptureId(id),
        owner_id: owner.to_string(),
        body: body.to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn stub_owned(id: common::id::CaptureId, owner: &str) -> Result<(), EmberTroveError> {
    if id.0 == Uuid::from_u128(AZ_OTHERS_CAPTURE) && owner != "someone-else" {
        return Err(EmberTroveError::NotFound("capture not found".to_string()));
    }
    Ok(())
}

struct StubNoteRepo;
#[async_trait]
impl NoteRepo for StubNoteRepo {
    async fn create(
        &self,
        _: NodeId,
        _: &str,
        _: CreateNoteRequest,
    ) -> Result<Note, EmberTroveError> {
        unimplemented!()
    }
    async fn update(
        &self,
        _: common::id::NoteId,
        _: &str,
        _: common::note::UpdateNoteRequest,
    ) -> Result<Note, EmberTroveError> {
        unimplemented!()
    }
    async fn list_for_node(&self, _: NodeId) -> Result<Vec<Note>, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: common::id::NoteId, _: &str) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn restore(
        &self,
        id: common::id::NoteId,
        owner_id: &str,
    ) -> Result<Note, EmberTroveError> {
        Ok(Note {
            id,
            node_id: NodeId(Uuid::nil()),
            owner_id: owner_id.to_string(),
            body: "restored".to_string(),
            color: "yellow".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
    }
    async fn purge_deleted_before(
        &self,
        _: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, EmberTroveError> {
        Ok(0)
    }
    async fn feed(
        &self,
        _: Option<&str>,
        _: &crate::repo::note::NoteFeedFilter,
    ) -> Result<Vec<FeedNote>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<Note>, EmberTroveError> {
        unimplemented!()
    }
}

struct StubAttachmentRepo;
#[async_trait]
impl AttachmentRepo for StubAttachmentRepo {
    async fn create(
        &self,
        _: NodeId,
        _: &str,
        _: &str,
        _: i64,
        _: &str,
    ) -> Result<Attachment, EmberTroveError> {
        unimplemented!()
    }
    async fn list(&self, _: NodeId) -> Result<Vec<Attachment>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<Attachment>, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, id: AttachmentId) -> Result<Attachment, EmberTroveError> {
        // The stub "database" holds no attachments — handlers must surface
        // this as 404 before any permission or S3 work.
        Err(EmberTroveError::NotFound(format!(
            "attachment {id} not found"
        )))
    }
    async fn delete(&self, _: AttachmentId) -> Result<String, EmberTroveError> {
        unimplemented!()
    }
}

struct StubPermissionRepo;
#[async_trait]
impl PermissionRepo for StubPermissionRepo {
    async fn grant(
        &self,
        _: NodeId,
        _: &str,
        _: GrantPermissionRequest,
    ) -> Result<Permission, EmberTroveError> {
        unimplemented!()
    }
    async fn revoke(&self, id: PermissionId, node: NodeId) -> Result<(), EmberTroveError> {
        match authz_permissions().into_iter().find(|p| p.id == id) {
            Some(p) if p.node_id == node && p.subject_id != "creator" => Ok(()),
            _ => panic!("authorization bypass: revoke reached the repo for {id}"),
        }
    }
    async fn list(&self, node: NodeId) -> Result<Vec<Permission>, EmberTroveError> {
        Ok(authz_permissions()
            .into_iter()
            .filter(|p| p.node_id == node)
            .collect())
    }
    async fn find(&self, node: NodeId, sub: &str) -> Result<Option<Permission>, EmberTroveError> {
        Ok(authz_permissions()
            .into_iter()
            .find(|p| p.node_id == node && p.subject_id == sub))
    }
    async fn list_all(&self, _: Option<NodeId>) -> Result<Vec<Permission>, EmberTroveError> {
        unimplemented!()
    }
    async fn find_by_id(&self, id: PermissionId) -> Result<Option<Permission>, EmberTroveError> {
        Ok(authz_permissions().into_iter().find(|p| p.id == id))
    }
    async fn update(
        &self,
        _: PermissionId,
        _: PermissionRole,
        _: &str,
    ) -> Result<Permission, EmberTroveError> {
        unimplemented!()
    }
}

struct StubSearchRepo;
#[async_trait]
impl SearchRepo for StubSearchRepo {
    async fn search(
        &self,
        _: &SearchQuery,
        _: Option<&str>,
    ) -> Result<SearchResponse, EmberTroveError> {
        unimplemented!()
    }
}

struct StubGraphRepo;
#[async_trait]
impl GraphRepo for StubGraphRepo {
    async fn list_positions(&self, _: Option<&str>) -> Result<Vec<NodePosition>, EmberTroveError> {
        unimplemented!()
    }
    async fn save_positions(
        &self,
        _: &[(Uuid, f64, f64)],
        _: Option<&str>,
    ) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

struct StubBackupRepo;
#[async_trait]
impl BackupRepo for StubBackupRepo {
    async fn create(
        &self,
        _: &str,
        _: &str,
        _: i64,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: Option<&str>,
    ) -> Result<BackupJob, EmberTroveError> {
        unimplemented!()
    }
    async fn list_for_owner(&self, _: &str) -> Result<Vec<BackupJob>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<BackupJob>, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, _: Uuid) -> Result<BackupJob, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: Uuid) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

struct StubFavoriteRepo;
#[async_trait]
impl FavoriteRepo for StubFavoriteRepo {
    async fn list_all(&self) -> Result<Vec<Favorite>, EmberTroveError> {
        unimplemented!()
    }
    async fn list(&self, _: &str) -> Result<Vec<Favorite>, EmberTroveError> {
        unimplemented!()
    }
    async fn create(
        &self,
        owner: &str,
        req: CreateFavoriteRequest,
    ) -> Result<Favorite, EmberTroveError> {
        Ok(Favorite {
            id: FavoriteId(Uuid::new_v4()),
            owner_id: owner.to_string(),
            node_id: req.node_id.map(NodeId),
            url: req.url,
            label: req.label,
            position: 0,
            created_at: chrono::Utc::now(),
        })
    }
    async fn delete(&self, _: FavoriteId, _: &str) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn reorder(&self, _: &str, _: &[FavoriteId]) -> Result<Vec<Favorite>, EmberTroveError> {
        unimplemented!()
    }
}

/// A minimal well-formed node for stubs that must return data (export tests).
fn fake_node(slug: &str, owner: &str) -> Node {
    Node {
        id: NodeId(Uuid::new_v4()),
        owner_id: owner.to_string(),
        node_type: NodeType::Article,
        title: format!("Title of {slug}"),
        slug: slug.to_string(),
        body: Some("body text".to_string()),
        metadata: serde_json::json!({}),
        status: NodeStatus::Published,
        project_status: None,
        tags: vec![],
        pinned: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        edge_count: 0,
    }
}

/// People stub for the handler tests (#147). The "registry" holds one
/// person, `AZ_PERSON`, owned by `test-sub`; any other id is a 404, the way
/// another owner's person is.
const AZ_PERSON: u128 = 0xa2_0e01;
/// The membership `add_membership` writes on AZ_MY_PROJECT (#216).
const AZ_NEW_MEMBERSHIP: u128 = 0xa2_0e10;

struct StubPersonRepo;
#[async_trait]
impl crate::repo::person::PersonRepo for StubPersonRepo {
    async fn list(
        &self,
        _: &str,
        _: Option<&str>,
        _: i64,
    ) -> Result<Vec<common::person::Person>, EmberTroveError> {
        Ok(vec![])
    }
    async fn get(
        &self,
        owner: &str,
        id: common::id::PersonId,
    ) -> Result<common::person::Person, EmberTroveError> {
        if owner == "test-sub" && id.0 == Uuid::from_u128(AZ_PERSON) {
            let now = chrono::Utc::now();
            Ok(common::person::Person {
                id,
                owner_id: owner.into(),
                display_name: "Ada".into(),
                company: None,
                position: None,
                emails: vec![],
                phones: vec![],
                addresses: vec![],
                notes: None,
                created_at: now,
                updated_at: now,
            })
        } else {
            Err(EmberTroveError::NotFound(format!("person {id} not found")))
        }
    }
    async fn create(
        &self,
        _: &str,
        _: &common::person::PersonInput,
    ) -> Result<common::person::Person, EmberTroveError> {
        unimplemented!()
    }
    async fn update(
        &self,
        _: &str,
        _: common::id::PersonId,
        _: &common::person::PersonInput,
    ) -> Result<common::person::Person, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: &str, _: common::id::PersonId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn find_by_emails(
        &self,
        _: &str,
        lowered: &[String],
    ) -> Result<Vec<common::person::DuplicateMatch>, EmberTroveError> {
        // Exactly one known address, so the duplicate warning can be tested.
        Ok(lowered
            .iter()
            .filter(|e| e.as_str() == "taken@example.com")
            .map(|e| common::person::DuplicateMatch {
                id: common::id::PersonId(Uuid::from_u128(AZ_PERSON)),
                display_name: "Ada".into(),
                email: e.clone(),
            })
            .collect())
    }
    async fn usages(
        &self,
        _: &str,
        _: common::id::PersonId,
    ) -> Result<Vec<common::person::PersonUsage>, EmberTroveError> {
        unimplemented!()
    }
    async fn role_counts(
        &self,
        _: &str,
    ) -> Result<Vec<common::person::RoleCount>, EmberTroveError> {
        Ok(vec![])
    }
    async fn memberships_for_node(
        &self,
        node: NodeId,
    ) -> Result<Vec<common::person::Membership>, EmberTroveError> {
        if node.0.as_u128() != AZ_MY_PROJECT {
            return Ok(vec![]);
        }
        let person = self
            .get("test-sub", common::id::PersonId(Uuid::from_u128(AZ_PERSON)))
            .await?;
        Ok(vec![common::person::Membership {
            id: common::id::MembershipId(Uuid::from_u128(AZ_NEW_MEMBERSHIP)),
            node_id: node,
            person,
            role: None,
            note: None,
            created_at: chrono::Utc::now(),
        }])
    }
    async fn add_membership(
        &self,
        node: NodeId,
        _: common::id::PersonId,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<common::id::MembershipId, EmberTroveError> {
        match node.0.as_u128() {
            AZ_MY_PROJECT => Ok(common::id::MembershipId(Uuid::from_u128(AZ_NEW_MEMBERSHIP))),
            other => panic!("capability bypass: membership added on node {other:#x}"),
        }
    }
    async fn update_membership(
        &self,
        _: NodeId,
        _: common::id::MembershipId,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn remove_membership(
        &self,
        _: NodeId,
        _: common::id::MembershipId,
    ) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(
        &self,
    ) -> Result<
        (
            Vec<common::person::Person>,
            Vec<common::person::MembershipRecord>,
        ),
        EmberTroveError,
    > {
        Ok((vec![], vec![]))
    }
}

struct StubActivityRepo;
#[async_trait]
impl ActivityRepo for StubActivityRepo {
    async fn record(
        &self,
        _: NodeId,
        _: &str,
        _: ActivityAction,
        _: serde_json::Value,
    ) -> Result<(), EmberTroveError> {
        Ok(())
    }
    async fn list(&self, _: NodeId, _: i64) -> Result<Vec<ActivityEntry>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_recent_for_owner(
        &self,
        _: &str,
        _: chrono::DateTime<chrono::Utc>,
        _: i64,
    ) -> Result<Vec<common::activity::RecentActivityEntry>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<common::activity::ActivityRecord>, EmberTroveError> {
        Ok(vec![])
    }
    async fn latest_of(
        &self,
        _: NodeId,
        _: ActivityAction,
    ) -> Result<Option<ActivityEntry>, EmberTroveError> {
        Ok(None)
    }
}

/// Records whose report was asked for, to prove the route never widens it.
#[derive(Default)]
struct StubDataHealthRepo(std::sync::Mutex<Vec<String>>);
#[async_trait]
impl crate::repo::data_health::DataHealthRepo for StubDataHealthRepo {
    async fn report_for(
        &self,
        owner: &str,
    ) -> Result<Vec<common::data_health::RuleReport>, EmberTroveError> {
        self.0.lock().expect("lock").push(owner.to_string());
        Ok(vec![])
    }
    async fn counts_all(&self) -> Result<Vec<common::data_health::RuleCount>, EmberTroveError> {
        Ok(vec![common::data_health::RuleCount {
            rule: common::data_health::RuleId::OrphanTask,
            count: 3,
        }])
    }
}

struct StubNodeVersionRepo;
#[async_trait]
impl NodeVersionRepo for StubNodeVersionRepo {
    async fn list_all(&self) -> Result<Vec<common::node_version::NodeVersion>, EmberTroveError> {
        unimplemented!()
    }
    async fn record(&self, _: NodeId, _: &str, _: &str) -> Result<(), EmberTroveError> {
        Ok(())
    }
    async fn list(
        &self,
        _: NodeId,
        _: i64,
    ) -> Result<Vec<common::node_version::NodeVersion>, EmberTroveError> {
        unimplemented!()
    }
    async fn get(
        &self,
        _: common::id::NodeVersionId,
    ) -> Result<common::node_version::NodeVersion, EmberTroveError> {
        unimplemented!()
    }
}

struct StubTemplateRepo;
#[async_trait]
impl TemplateRepo for StubTemplateRepo {
    async fn list(&self, _: Option<&str>) -> Result<Vec<NodeTemplate>, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, _: TemplateId) -> Result<NodeTemplate, EmberTroveError> {
        unimplemented!()
    }
    async fn create(
        &self,
        _: &str,
        _: CreateTemplateRequest,
    ) -> Result<NodeTemplate, EmberTroveError> {
        unimplemented!()
    }
    async fn update(
        &self,
        _: TemplateId,
        _: UpdateTemplateRequest,
    ) -> Result<NodeTemplate, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: TemplateId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
    async fn set_default(&self, _: TemplateId, _: &str) -> Result<NodeTemplate, EmberTroveError> {
        unimplemented!()
    }
}

struct StubNodeLinkRepo;
#[async_trait]
impl NodeLinkRepo for StubNodeLinkRepo {
    async fn list_all(&self) -> Result<Vec<NodeLink>, EmberTroveError> {
        unimplemented!()
    }
    async fn list(&self, _: NodeId) -> Result<Vec<NodeLink>, EmberTroveError> {
        unimplemented!()
    }
    async fn create(
        &self,
        _: NodeId,
        _: CreateNodeLinkRequest,
    ) -> Result<NodeLink, EmberTroveError> {
        unimplemented!()
    }
    async fn update(
        &self,
        _: NodeId,
        _: NodeLinkId,
        _: UpdateNodeLinkRequest,
    ) -> Result<NodeLink, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: NodeId, _: NodeLinkId) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

struct StubSearchPresetRepo;
#[async_trait]
impl SearchPresetRepo for StubSearchPresetRepo {
    async fn list(&self, _: &str) -> Result<Vec<SearchPreset>, EmberTroveError> {
        unimplemented!()
    }
    async fn create(
        &self,
        _: &str,
        _: CreateSearchPresetRequest,
    ) -> Result<SearchPreset, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: SearchPresetId, _: &str) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

struct StubWebhookRepo;
#[async_trait]
impl WebhookRepo for StubWebhookRepo {
    async fn list(&self, _: &str) -> Result<Vec<Webhook>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_active_for_event(&self, _: &str) -> Result<Vec<Webhook>, EmberTroveError> {
        // Handlers fire-and-forget webhook dispatch; nothing is subscribed.
        Ok(vec![])
    }
    async fn create(&self, _: &str, _: CreateWebhookRequest) -> Result<Webhook, EmberTroveError> {
        unimplemented!()
    }
    async fn update(
        &self,
        _: WebhookId,
        _: &str,
        _: UpdateWebhookRequest,
    ) -> Result<Webhook, EmberTroveError> {
        unimplemented!()
    }
    async fn delete(&self, _: WebhookId, _: &str) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

struct StubEditorPrefRepo;
#[async_trait]
impl crate::repo::editor_pref::EditorPrefRepo for StubEditorPrefRepo {
    async fn list_for_owner(
        &self,
        _: &str,
    ) -> Result<Vec<common::editor_pref::EditorPref>, EmberTroveError> {
        Ok(vec![])
    }
    async fn set(
        &self,
        _: &str,
        _: &str,
        _: Uuid,
        _: Option<i32>,
        _: Option<bool>,
    ) -> Result<(), EmberTroveError> {
        Ok(())
    }
    async fn set_collapsed_many(
        &self,
        _: &str,
        _: &str,
        _: &[Uuid],
        _: bool,
    ) -> Result<(), EmberTroveError> {
        Ok(())
    }
}

struct StubPkceRepo;
#[async_trait]
impl PkceRepo for StubPkceRepo {
    async fn store(&self, _: &str, _: &str) -> Result<(), EmberTroveError> {
        Ok(())
    }
    async fn take(&self, _: &str, _: Duration) -> Result<Option<String>, EmberTroveError> {
        Ok(None)
    }
    async fn sweep_expired(&self, _: Duration) -> Result<u64, EmberTroveError> {
        Ok(0)
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────────

fn test_state() -> AppState {
    // Short acquire_timeout ensures the health-check DB query fails fast (~50 ms)
    // rather than blocking until the 30-second TimeoutLayer fires.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(50))
        .connect_lazy("postgres://test:test@127.0.0.1:5432/test")
        .expect("lazy pool from valid URL");

    AppState {
        pool,
        nodes: Arc::new(StubNodeRepo),
        edges: Arc::new(StubEdgeRepo),
        tags: Arc::new(StubTagRepo),
        tasks: Arc::new(StubTaskRepo),
        notes: Arc::new(StubNoteRepo),
        captures: Arc::new(StubCaptureRepo),
        attachments: Arc::new(StubAttachmentRepo),
        permissions: Arc::new(StubPermissionRepo),
        search: Arc::new(StubSearchRepo),
        graph: Arc::new(StubGraphRepo),
        backup: Arc::new(StubBackupRepo),
        favorites: Arc::new(StubFavoriteRepo),
        activity: Arc::new(StubActivityRepo),
        data_health: Arc::new(StubDataHealthRepo::default()),
        people: Arc::new(StubPersonRepo),
        node_versions: Arc::new(StubNodeVersionRepo),
        templates: Arc::new(StubTemplateRepo),
        search_presets: Arc::new(StubSearchPresetRepo),
        node_links: Arc::new(StubNodeLinkRepo),
        webhooks: Arc::new(StubWebhookRepo),
        editor_prefs: Arc::new(StubEditorPrefRepo),
        accounts: Arc::new(StubAccountRepo::default()),
        suspended: crate::auth::suspended::SuspendedSet::default(),
        object_store: Arc::new(NullObjectStore),
        oidc: None,
        cognito_admin: None,
        notifier: None,
        cookie_key: Key::generate(),
        auth: AuthConfig {
            issuer: "https://example.com".to_string(),
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_external_url: "http://localhost:3003".to_string(),
            cookie_secure: false,
        },
        config: Config {
            database_url: "postgres://test:test@127.0.0.1:5432/test".to_string(),
            ..Config::default()
        },
        started_at: Instant::now(),
        request_metrics: Arc::new(crate::routes::metrics::RequestMetrics::default()),
        pkce: Arc::new(StubPkceRepo),
    }
}

fn test_app() -> Router {
    build_router(test_state()).expect("test router must build successfully")
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("response body must be valid JSON")
}

/// Verify a route is registered: build a request and assert the status is not 404.
/// With oidc = None the auth middleware returns 500 before any handler runs, so:
///   - 404 → route not registered
///   - anything else → route exists
async fn assert_route_registered(method: &str, uri: &str) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-forwarded-for", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let status = test_app().oneshot(req).await.unwrap().status();
    assert_ne!(
        status,
        StatusCode::NOT_FOUND,
        "{method} {uri} must be registered (got {status})"
    );
}

// ── Health endpoint ───────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_200() {
    let resp = test_app()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn request_metrics_middleware_counts_responses() {
    // Inject a shared counter so we can read it after driving requests through
    // the real router — proves the track_requests middleware is wired in.
    let metrics = Arc::new(crate::routes::metrics::RequestMetrics::default());
    let mut state = test_state();
    state.request_metrics = metrics.clone();
    let app = build_router(state).expect("router builds");

    for _ in 0..3 {
        let _ = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
    }

    let snap = metrics.snapshot();
    assert_eq!(snap.total, 3, "all three /health responses must be counted");
    assert_eq!(snap.status_2xx, 3, "/health returns 200 → 2xx bucket");
}

#[tokio::test]
async fn health_body_has_required_fields() {
    let resp = test_app()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = body_json(resp).await;
    for field in ["status", "service", "version", "database"] {
        assert!(body.get(field).is_some(), "health body must have '{field}'");
    }
}

#[tokio::test]
async fn health_version_matches_cargo_pkg() {
    let resp = test_app()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(
        body["version"].as_str().unwrap(),
        env!("CARGO_PKG_VERSION"),
        "health 'version' must equal CARGO_PKG_VERSION"
    );
}

#[tokio::test]
async fn health_service_field_is_correct() {
    let resp = test_app()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["service"], "ember-trove-api");
}

// ── Routing fallthrough ───────────────────────────────────────────────────────
// In Axum 0.8, `Router::layer()` wraps the entire router service, so the
// `require_auth` middleware fires even for unmatched routes.  With oidc = None
// unmatched requests return 500 (OIDC not configured), not 404.  The important
// invariant is that the health endpoint is unaffected and still returns 200.

#[tokio::test]
async fn unknown_route_does_not_return_success() {
    let resp = test_app()
        .oneshot(
            Request::get("/this-path-does-not-exist")
                .header("x-forwarded-for", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !resp.status().is_success(),
        "unknown route must not return a 2xx status; got {}",
        resp.status()
    );
}

// ── Auth guard ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn protected_route_without_oidc_returns_500_with_error_field() {
    let resp = test_app()
        .oneshot(
            Request::get("/nodes")
                .header("x-forwarded-for", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(resp).await;
    assert!(
        body.get("error").is_some(),
        "auth-guard error response must have 'error' field; got: {body}"
    );
}

// ── Node routes ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn nodes_list_route_registered() {
    assert_route_registered("GET", "/nodes").await;
    assert_route_registered("GET", "/nodes/project-statuses").await;
}
#[tokio::test]
async fn node_create_route_registered() {
    assert_route_registered("POST", "/nodes").await;
}
#[tokio::test]
async fn node_get_route_registered() {
    assert_route_registered("GET", &format!("/nodes/{}", Uuid::new_v4())).await;
}
#[tokio::test]
async fn node_update_route_registered() {
    assert_route_registered("PUT", &format!("/nodes/{}", Uuid::new_v4())).await;
}
#[tokio::test]
async fn node_delete_route_registered() {
    assert_route_registered("DELETE", &format!("/nodes/{}", Uuid::new_v4())).await;
}

// ── Per-node permission routes ────────────────────────────────────────────────

#[tokio::test]
async fn node_permissions_list_route_registered() {
    let nid = Uuid::new_v4();
    assert_route_registered("GET", &format!("/nodes/{nid}/permissions")).await;
}
#[tokio::test]
async fn node_permissions_grant_route_registered() {
    let nid = Uuid::new_v4();
    assert_route_registered("POST", &format!("/nodes/{nid}/permissions")).await;
}
#[tokio::test]
async fn node_permissions_revoke_route_registered() {
    let nid = Uuid::new_v4();
    let pid = Uuid::new_v4();
    assert_route_registered("DELETE", &format!("/nodes/{nid}/permissions/{pid}")).await;
}

// ── Standalone permission routes (v1.21.0) ────────────────────────────────────

#[tokio::test]
async fn standalone_permissions_list_route_registered() {
    assert_route_registered("GET", "/permissions").await;
}
#[tokio::test]
async fn standalone_permission_update_route_registered() {
    assert_route_registered("PUT", &format!("/permissions/{}", Uuid::new_v4())).await;
}
#[tokio::test]
async fn standalone_permission_delete_route_registered() {
    assert_route_registered("DELETE", &format!("/permissions/{}", Uuid::new_v4())).await;
}

// ── Other domain routes ───────────────────────────────────────────────────────

#[tokio::test]
async fn tags_list_route_registered() {
    assert_route_registered("GET", "/tags").await;
}
#[tokio::test]
async fn search_route_registered() {
    assert_route_registered("GET", "/search").await;
}
#[tokio::test]
async fn graph_route_registered() {
    assert_route_registered("GET", "/graph").await;
}
#[tokio::test]
async fn favorites_list_route_registered() {
    assert_route_registered("GET", "/favorites").await;
}
#[tokio::test]
async fn notes_feed_route_registered() {
    assert_route_registered("GET", "/notes").await;
}
#[tokio::test]
async fn edges_list_route_registered() {
    assert_route_registered("GET", "/edges").await;
}

// ── Template routes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn templates_list_route_registered() {
    assert_route_registered("GET", "/templates").await;
}
#[tokio::test]
async fn template_create_route_registered() {
    assert_route_registered("POST", "/templates").await;
}
#[tokio::test]
async fn template_get_route_registered() {
    assert_route_registered("GET", &format!("/templates/{}", Uuid::new_v4())).await;
}
#[tokio::test]
async fn template_update_route_registered() {
    assert_route_registered("PUT", &format!("/templates/{}", Uuid::new_v4())).await;
}
#[tokio::test]
async fn template_delete_route_registered() {
    assert_route_registered("DELETE", &format!("/templates/{}", Uuid::new_v4())).await;
}

// ── Node pin route ─────────────────────────────────────────────────────────────
#[tokio::test]
async fn node_pin_route_registered() {
    assert_route_registered("PUT", &format!("/nodes/{}/pin", Uuid::new_v4())).await;
}

// ── Permission DTO unit tests ─────────────────────────────────────────────────

#[test]
fn update_permission_request_serializes_role() {
    use common::permission::UpdatePermissionRequest;
    let req = UpdatePermissionRequest {
        role: PermissionRole::Editor,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["role"], "editor");
}

#[test]
fn permission_role_roundtrip_all_variants() {
    for (variant, expected) in [
        (PermissionRole::Owner, "owner"),
        (PermissionRole::Editor, "editor"),
        (PermissionRole::Viewer, "viewer"),
    ] {
        let s = serde_json::to_string(&variant).unwrap();
        assert_eq!(s, format!("\"{expected}\""), "serialize {expected}");
        let back: PermissionRole = serde_json::from_str(&s).unwrap();
        assert_eq!(back, variant, "deserialize {expected}");
    }
}

// ── Search presets routes ──────────────────────────────────────────────────────

#[tokio::test]
async fn search_preset_list_route_registered() {
    assert_route_registered("GET", "/api/search-presets").await;
}

#[tokio::test]
async fn search_preset_create_route_registered() {
    assert_route_registered("POST", "/api/search-presets").await;
}

#[tokio::test]
async fn search_preset_delete_route_registered() {
    let id = Uuid::new_v4();
    assert_route_registered("DELETE", &format!("/api/search-presets/{id}")).await;
}

// ── Quick capture (Phase 1 / iOS Web Share Target) ───────────────────────────

#[tokio::test]
async fn inbox_quick_route_registered() {
    assert_route_registered("POST", "/inbox/quick").await;
}

// ── My Day Kanban backlog feed (Phase 4 / v2.6.0) ────────────────────────────

#[tokio::test]
async fn tasks_all_open_route_registered() {
    assert_route_registered("GET", "/tasks/all").await;
}

// ── Soft delete + undo (restore endpoints) ────────────────────────────────────
// `assert_route_registered` is vacuous for nested paths: the auth layer answers
// 500 before route matching, so even an unregistered path passes the !=404
// assertion (verified 2026-06-10; see .claude/ERRORS.md). These tests instead
// drive the real handlers through a bare router slice with claims injected.

fn fake_claims() -> common::auth::AuthClaims {
    common::auth::AuthClaims {
        sub: "test-sub".to_string(),
        email: None,
        name: None,
        roles: vec![],
        exp: i64::MAX,
    }
}

#[tokio::test]
async fn task_restore_returns_restored_task_for_owner() {
    let app = Router::new()
        .nest("/tasks", crate::routes::tasks::task_router())
        .layer(axum::Extension(fake_claims()))
        .with_state(test_state());
    let id = Uuid::new_v4();
    let resp = app
        .oneshot(
            Request::post(format!("/tasks/{id}/restore"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["id"], id.to_string());
}

#[tokio::test]
async fn note_restore_returns_restored_note_for_owner() {
    let app = Router::new()
        .nest("/notes", crate::routes::notes::note_router())
        .layer(axum::Extension(fake_claims()))
        .with_state(test_state());
    let id = Uuid::new_v4();
    let resp = app
        .oneshot(
            Request::post(format!("/notes/{id}/restore"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["id"], id.to_string());
    // Restore is owner-scoped: the stub echoes the owner the handler passed,
    // which must be the caller's sub.
    assert_eq!(json["owner_id"], "test-sub");
}

// ── Dashboard recent activity recap (Phase 7 / v2.9.0) ───────────────────────

#[tokio::test]
async fn dashboard_activity_route_registered() {
    assert_route_registered("GET", "/dashboard/activity").await;
}

// ── OAuth callback failure handling ───────────────────────────────────────────
// /auth/callback is a browser-navigation endpoint: Cognito redirects the user's
// browser there after login. On failure (missing PKCE verifier, Cognito reject,
// OIDC misconfig) the handler must respond with a 303 redirect to the frontend
// — NOT the shared `ApiError` JSON response, which a browser would render as
// the page itself. See feedback_browser_navigation_endpoints.md.

#[tokio::test]
async fn auth_callback_redirects_on_misconfig_instead_of_json_500() {
    // test_state() has oidc = None, which would make try_callback return
    // ApiError::Internal. The outer handler must turn that into a 303 redirect.
    let resp = test_app()
        .oneshot(
            Request::get("/auth/callback?code=anything&state=missing")
                .header("x-forwarded-for", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "callback failures must redirect (303), not return a JSON error page"
    );
    let location = resp
        .headers()
        .get(axum::http::header::LOCATION)
        .expect("303 must include a Location header");
    assert_eq!(
        location, "http://localhost:3000",
        "redirect target must be the frontend root (test AuthConfig.frontend_url)"
    );
    assert_ne!(
        resp.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap_or("")),
        Some("application/json"),
        "callback failure response must not be JSON"
    );
}

#[tokio::test]
async fn auth_callback_redirects_when_state_param_missing() {
    // No `state` query param at all → no PKCE lookup possible → must redirect.
    let resp = test_app()
        .oneshot(
            Request::get("/auth/callback?code=anything")
                .header("x-forwarded-for", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
}

// ── Share-token security (v2.22.4 hardening) ─────────────────────────────────

fn admin_claims() -> common::auth::AuthClaims {
    common::auth::AuthClaims {
        roles: vec!["admin".to_string()],
        ..fake_claims()
    }
}

// ── Public share links are gone (#147) ────────────────────────────────────────

/// Owner decision (#147): public share links were removed entirely. They
/// were unauthenticated read access to a node, and too easy to publish by
/// accident. No old URL may reach a handler: each is either unrouted (404)
/// or stopped by the auth layer that wraps everything that isn't public. In
/// this harness that layer answers 500, because there is no OIDC, and in
/// production it answers 401. Neither has a share handler's body.
#[tokio::test]
async fn public_share_link_routes_are_gone() {
    let node = Uuid::new_v4();
    let token = Uuid::new_v4();
    for (method, uri) in [
        ("GET", format!("/share/{token}")),
        ("GET", format!("/nodes/{node}/share")),
        ("POST", format!("/nodes/{node}/share")),
        ("DELETE", format!("/nodes/{node}/share/{token}")),
        ("DELETE", format!("/share/{token}")),
    ] {
        let req = Request::builder()
            .method(method)
            .uri(&uri)
            .header("x-forwarded-for", "127.0.0.1")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let resp = test_app().oneshot(req).await.unwrap();
        let status = resp.status();
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::INTERNAL_SERVER_ERROR,
            "{method} {uri} must be unrouted or behind auth, got {status}"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&body).to_lowercase();
        assert!(
            !body.contains("share"),
            "{method} {uri} reached a share handler: {body}"
        );
    }
}

// ── Privileged & previously-untested route groups (v2.23.0) ──────────────────
// admin, backup, metrics, export, attachments, editor-prefs had no tests at
// all — the most privileged surfaces in the app. Registration first, then
// behavior where the stubs allow it.

#[tokio::test]
async fn admin_routes_registered() {
    assert_route_registered("GET", "/admin/users").await;
    assert_route_registered("POST", "/admin/users").await;
    assert_route_registered("GET", "/admin/users/roles").await;
    let id = Uuid::new_v4();
    assert_route_registered("DELETE", &format!("/admin/users/{id}")).await;
    assert_route_registered("PUT", &format!("/admin/users/{id}/roles")).await;
}

#[tokio::test]
async fn backup_routes_registered() {
    assert_route_registered("GET", "/admin/backups").await;
    assert_route_registered("POST", "/admin/backups").await;
    let id = Uuid::new_v4();
    assert_route_registered("DELETE", &format!("/admin/backups/{id}")).await;
    assert_route_registered("GET", &format!("/admin/backups/{id}/download")).await;
    assert_route_registered("GET", &format!("/admin/backups/{id}/preview")).await;
    assert_route_registered("POST", &format!("/admin/backups/{id}/restore")).await;
}

#[tokio::test]
async fn metrics_export_attachments_editor_prefs_routes_registered() {
    assert_route_registered("GET", "/metrics").await;
    assert_route_registered("GET", "/export").await;
    assert_route_registered("GET", "/data-health").await;
    let id = Uuid::new_v4();
    assert_route_registered("GET", &format!("/attachments/{id}/download")).await;
    assert_route_registered("DELETE", &format!("/attachments/{id}")).await;
    assert_route_registered("GET", "/editor-prefs").await;
    assert_route_registered("PUT", "/editor-prefs").await;
}

/// Mini-router around one protected sub-router with injected (non-admin) claims.
fn claims_app(path: &str, router: Router<crate::state::AppState>) -> Router {
    Router::new()
        .nest(path, router)
        .layer(axum::Extension(fake_claims()))
        .with_state(test_state())
}

#[tokio::test]
async fn admin_users_rejects_non_admin() {
    let resp = claims_app("/admin", crate::routes::admin::router())
        .oneshot(Request::get("/admin/users").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn backup_list_and_restore_reject_non_admin() {
    // The two most dangerous backup endpoints: read the archive list and
    // execute a restore. Both must 403 for a non-admin before any work runs.
    let app = claims_app("/admin/backups", crate::routes::backup::router());
    let resp = app
        .clone()
        .oneshot(Request::get("/admin/backups").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let id = Uuid::new_v4();
    let resp = app
        .oneshot(
            Request::post(format!("/admin/backups/{id}/restore"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// One stored backup job (#222 restore-order test).
struct OneJobBackupRepo(BackupJob);

#[async_trait]
impl BackupRepo for OneJobBackupRepo {
    async fn create(
        &self,
        _: &str,
        _: &str,
        _: i64,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: Option<&str>,
    ) -> Result<BackupJob, EmberTroveError> {
        panic!("the pre-restore snapshot ran for an archive restore refuses")
    }
    async fn list_for_owner(&self, _: &str) -> Result<Vec<BackupJob>, EmberTroveError> {
        unimplemented!()
    }
    async fn list_all(&self) -> Result<Vec<BackupJob>, EmberTroveError> {
        unimplemented!()
    }
    async fn get(&self, _: Uuid) -> Result<BackupJob, EmberTroveError> {
        Ok(self.0.clone())
    }
    async fn delete(&self, _: Uuid) -> Result<(), EmberTroveError> {
        unimplemented!()
    }
}

/// Serves one archive for every key and records every upload.
struct ArchiveStore {
    archive: bytes::Bytes,
    puts: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl crate::object_store::ObjectStore for ArchiveStore {
    async fn put(&self, key: &str, _: bytes::Bytes, _: &str) -> Result<(), EmberTroveError> {
        self.puts.lock().unwrap().push(key.to_string());
        Ok(())
    }
    async fn get(&self, _: &str) -> Result<bytes::Bytes, EmberTroveError> {
        Ok(self.archive.clone())
    }
    async fn delete(&self, _: &str) -> Result<(), EmberTroveError> {
        Ok(())
    }
    async fn presigned_url(&self, _: &str, _: u32) -> Result<String, EmberTroveError> {
        unimplemented!()
    }
}

/// A tar.gz holding `manifest.json` at `version` and a `data.json` that
/// isn't JSON, so reaching the parser would fail differently.
fn archive_at(version: u32) -> bytes::Bytes {
    let manifest = serde_json::json!({
        "schema_version": version,
        "created_at": "2026-10-01T00:00:00Z",
        "created_by": "admin",
        "entity_counts": {"nodes": 1, "edges": 0, "tags": 0, "notes": 0, "tasks": 0, "attachments": 0}
    })
    .to_string();
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (path, data) in [
        ("manifest.json", manifest.as_bytes()),
        ("data.json", b"not json".as_slice()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, data).unwrap();
    }
    bytes::Bytes::from(builder.into_inner().unwrap().finish().unwrap())
}

#[tokio::test]
async fn restore_refuses_an_old_backup_before_the_snapshot() {
    // #222: a format-2 archive is refused with a 422 before the pre-restore
    // snapshot (the repo's create panics, the store records no upload) and
    // before data.json is parsed (it isn't JSON here).
    let store = Arc::new(ArchiveStore {
        archive: archive_at(2),
        puts: std::sync::Mutex::new(Vec::new()),
    });
    let job = BackupJob {
        id: Uuid::new_v4(),
        created_by: "admin".into(),
        created_at: chrono::Utc::now(),
        size_bytes: 0,
        s3_key: "backups/old.tar.gz".into(),
        node_count: 1,
        edge_count: 0,
        tag_count: 0,
        note_count: 0,
        task_count: 0,
        attachment_count: 0,
        comment: None,
    };
    let state = AppState {
        backup: Arc::new(OneJobBackupRepo(job.clone())),
        object_store: store.clone(),
        ..test_state()
    };
    let res = crate::backup::execute_restore(&state, job.id, "admin").await;
    assert!(
        matches!(&res, Err(crate::error::ApiError::Validation(m)) if m.contains("older version")),
        "got {res:?}"
    );
    let preview = crate::backup::preview_restore(&state, job.id, "admin").await;
    assert!(
        matches!(&preview, Err(crate::error::ApiError::Validation(m)) if m.contains("format 2")),
        "the preview says so up front: {preview:?}"
    );
    assert!(
        store.puts.lock().unwrap().is_empty(),
        "no snapshot uploaded"
    );
}

#[tokio::test]
async fn metrics_rejects_non_admin() {
    let resp = claims_app("/metrics", crate::routes::metrics::router())
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

/// Count the entries in a ZIP body and return their names.
fn zip_entry_names(bytes: &[u8]) -> Vec<String> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).expect("valid zip archive");
    (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .expect("zip entry")
                .name()
                .expect("valid entry name") // zip 9: name() validates and returns Result
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn export_scopes_to_owned_nodes_for_non_admin() {
    // The stub returns 1 node for the owner and 2 for list_all — a non-admin
    // export must contain exactly the owner's node.
    let resp = claims_app("/export", crate::routes::export::router())
        .oneshot(Request::get("/export").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap(),
        "application/zip"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(zip_entry_names(&body), vec!["owned-node.md"]);
}

#[tokio::test]
async fn export_includes_all_nodes_for_admin() {
    let app = Router::new()
        .nest("/export", crate::routes::export::router())
        .layer(axum::Extension(admin_claims()))
        .with_state(test_state());
    let resp = app
        .oneshot(Request::get("/export").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let names = zip_entry_names(&body);
    assert_eq!(names.len(), 2, "admin export must include all nodes");
    assert!(names.contains(&"other-users-node.md".to_string()));
}

#[tokio::test]
async fn attachment_download_and_delete_unknown_id_return_not_found() {
    // The stub holds no attachments: both handlers must 404 from the lookup,
    // before permissions or S3 are consulted.
    let app = claims_app("/attachments", crate::routes::attachments::router());
    let id = Uuid::new_v4();
    let resp = app
        .clone()
        .oneshot(
            Request::get(format!("/attachments/{id}/download"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = app
        .oneshot(
            Request::delete(format!("/attachments/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn editor_prefs_list_returns_json_array() {
    let resp = claims_app("/editor-prefs", crate::routes::editor_prefs::router())
        .oneshot(Request::get("/editor-prefs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_array());
}

#[tokio::test]
async fn editor_prefs_set_validates_and_accepts() {
    let app = claims_app("/editor-prefs", crate::routes::editor_prefs::router());
    let put = |body: &str| {
        Request::put("/editor-prefs")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let id = Uuid::new_v4();
    // Valid task pref → 204.
    let resp = app
        .clone()
        .oneshot(put(&format!(
            r#"{{"entity_kind":"task","entity_id":"{id}","height":240}}"#
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    // Unknown entity kind → 422 (ApiError::Validation).
    let resp = app
        .clone()
        .oneshot(put(&format!(
            r#"{{"entity_kind":"widget","entity_id":"{id}","height":240}}"#
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // Height outside the 60–4000 clamp → 422.
    let resp = app
        .oneshot(put(&format!(
            r#"{{"entity_kind":"note","entity_id":"{id}","height":12}}"#
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn editor_prefs_set_collapse_needs_a_field_and_a_note() {
    let app = claims_app("/editor-prefs", crate::routes::editor_prefs::router());
    let put = |body: String| {
        Request::put("/editor-prefs")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    let id = Uuid::new_v4();
    // Collapse-only pref for a note (no height) → 204.
    let resp = app
        .clone()
        .oneshot(put(format!(
            r#"{{"entity_kind":"note","entity_id":"{id}","collapsed":true}}"#
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    // Neither height nor collapsed → 422 (nothing to save).
    let resp = app
        .clone()
        .oneshot(put(format!(
            r#"{{"entity_kind":"note","entity_id":"{id}"}}"#
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // Collapse is a note-only pref → 422 for tasks.
    let resp = app
        .oneshot(put(format!(
            r#"{{"entity_kind":"task","entity_id":"{id}","collapsed":true}}"#
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn editor_prefs_bulk_collapse_validates_batch() {
    let app = claims_app("/editor-prefs", crate::routes::editor_prefs::router());
    let put = |body: String| {
        Request::put("/editor-prefs/collapsed")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    let ids = |n: usize| {
        (0..n)
            .map(|_| format!("\"{}\"", Uuid::new_v4()))
            .collect::<Vec<_>>()
            .join(",")
    };
    // A normal batch of notes → 204.
    let resp = app
        .clone()
        .oneshot(put(format!(
            r#"{{"entity_kind":"note","entity_ids":[{}],"collapsed":true}}"#,
            ids(3)
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    // Empty batch → 422.
    let resp = app
        .clone()
        .oneshot(put(
            r#"{"entity_kind":"note","entity_ids":[],"collapsed":false}"#.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // Over the MAX_BULK_COLLAPSE cap → 422.
    let resp = app
        .clone()
        .oneshot(put(format!(
            r#"{{"entity_kind":"note","entity_ids":[{}],"collapsed":true}}"#,
            ids(common::editor_pref::MAX_BULK_COLLAPSE + 1)
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // Collapse is note-only → 422 for tasks.
    let resp = app
        .oneshot(put(format!(
            r#"{{"entity_kind":"task","entity_ids":[{}],"collapsed":true}}"#,
            ids(1)
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── Handler-level authorization (v2.31.1 fixes) ──────────────────────────────
// The repo-level fixes have Postgres tests; these prove the *handlers* make
// the right decision before any write (see "Authorization world" above).

fn patch_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::patch(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn task_patch(task: u128, body: serde_json::Value) -> StatusCode {
    claims_app("/tasks", crate::routes::tasks::task_router())
        .oneshot(patch_json(
            &format!("/tasks/{}", Uuid::from_u128(task)),
            body,
        ))
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn update_task_owner_can_edit_their_own_task() {
    let status = task_patch(0xa2_0199, serde_json::json!({ "title": "renamed" })).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn update_task_cannot_move_a_task_onto_a_node_without_access() {
    // Regression (v2.31.1): only the task's *current* node was checked, so
    // an owner could re-parent their task onto any node id.
    let dest = Uuid::from_u128(AZ_FOREIGN_NODE);
    let status = task_patch(0xa2_0199, serde_json::json!({ "node_id": dest })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_task_cannot_move_a_task_onto_a_view_only_node() {
    let dest = Uuid::from_u128(AZ_VIEW_NODE);
    let status = task_patch(0xa2_0199, serde_json::json!({ "node_id": dest })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_task_can_move_a_task_onto_an_editable_project() {
    let dest = Uuid::from_u128(AZ_MY_PROJECT);
    let status = task_patch(0xa2_0199, serde_json::json!({ "node_id": dest })).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn update_task_refuses_a_move_onto_an_editable_non_project() {
    // §3.2 (#216): editor access isn't enough; only a Project takes tasks.
    let dest = Uuid::from_u128(AZ_EDIT_NODE);
    let status = task_patch(0xa2_0199, serde_json::json!({ "node_id": dest })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn update_task_refuses_someone_elses_standalone_task() {
    let status = task_patch(AZ_FOREIGN_TASK, serde_json::json!({ "title": "x" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_task_on_a_shared_node_needs_editor_not_viewer() {
    let viewer = task_patch(AZ_VIEW_TASK, serde_json::json!({ "title": "x" })).await;
    assert_eq!(viewer, StatusCode::FORBIDDEN);
    let editor = task_patch(AZ_EDIT_TASK, serde_json::json!({ "title": "x" })).await;
    assert_eq!(editor, StatusCode::OK);
}

async fn nested_revoke(node: u128, perm: u128) -> StatusCode {
    claims_app("/nodes", crate::routes::nodes::router())
        .oneshot(
            Request::delete(format!(
                "/nodes/{}/permissions/{}",
                Uuid::from_u128(node),
                Uuid::from_u128(perm)
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn revoke_permission_removes_a_grant_on_the_callers_node() {
    assert_eq!(
        nested_revoke(AZ_OWNED_NODE, AZ_PERM_GUEST).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn revoke_permission_is_scoped_to_the_path_node() {
    // Regression (v2.31.1, permission revoke IDOR): an owner of one node could
    // revoke a permission row of any other node by id.
    assert_eq!(
        nested_revoke(AZ_OWNED_NODE, AZ_PERM_FOREIGN).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn revoke_permission_cannot_remove_the_canonical_owner() {
    // A *granted* Owner must not lock the node's creator out.
    assert_eq!(
        nested_revoke(AZ_OWNED_NODE, AZ_PERM_CREATOR).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn standalone_permission_delete_applies_the_same_checks() {
    let delete = |perm: u128| async move {
        claims_app("/permissions", crate::routes::permissions::router())
            .oneshot(
                Request::delete(format!("/permissions/{}", Uuid::from_u128(perm)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    };
    // Not an owner of the foreign node.
    assert_eq!(delete(AZ_PERM_FOREIGN).await, StatusCode::FORBIDDEN);
    // Canonical-owner backstop (the standalone route used to skip it).
    assert_eq!(delete(AZ_PERM_CREATOR).await, StatusCode::FORBIDDEN);
    assert_eq!(delete(AZ_PERM_GUEST).await, StatusCode::NO_CONTENT);
}

// ── GET /nodes/lookup (node pickers, #126 follow-up) ─────────────────────────

async fn lookup(claims: common::auth::AuthClaims, query: &str) -> (StatusCode, serde_json::Value) {
    let resp = Router::new()
        .nest("/nodes", crate::routes::nodes::router())
        .layer(axum::Extension(claims))
        .with_state(test_state())
        .oneshot(
            Request::get(format!("/nodes/lookup?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn node_lookup_scopes_to_the_caller_and_clamps_the_limit() {
    let (status, json) = lookup(fake_claims(), "q=Phy").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json[0]["title"], "test-sub|Phy|8",
        "caller's scope, default 8"
    );
    let (_, json) = lookup(fake_claims(), "q=Phy&limit=500").await;
    assert_eq!(json[0]["title"], "test-sub|Phy|20", "limit clamped");
    let (_, json) = lookup(admin_claims(), "q=Phy").await;
    assert_eq!(json[0]["title"], "<admin>|Phy|8", "admin: every node");
}

#[tokio::test]
async fn node_lookup_passes_a_kind_and_rejects_an_unknown_one() {
    // #216: the task pickers ask for Projects only.
    let (status, json) = lookup(fake_claims(), "q=Phy&node_type=project").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json[0]["node_type"], "project");
    let (status, _) = lookup(fake_claims(), "q=Phy&node_type=nonsense").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "not a node kind");
    // #222: Resource merged into Reference; the API no longer knows it.
    let (status, _) = lookup(fake_claims(), "q=Phy&node_type=resource").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "resource is retired");
}

#[tokio::test]
async fn node_lookup_rejects_an_oversized_query() {
    let long = "a".repeat(201);
    let (status, _) = lookup(fake_claims(), &format!("q={long}")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ── POST /edges duplicate (#129) ─────────────────────────────────────────────

async fn post_edge(target: u128) -> (StatusCode, serde_json::Value) {
    post_typed_edge(AZ_EDIT_NODE, target, "references").await
}

async fn post_typed_edge(
    source: u128,
    target: u128,
    edge_type: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = claims_app("/edges", crate::routes::edges::router())
        .oneshot(
            Request::post("/edges")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "source_id": Uuid::from_u128(source),
                        "target_id": Uuid::from_u128(target),
                        "edge_type": edge_type,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn create_edge_reports_a_duplicate_as_409() {
    let (status, _) = post_edge(AZ_OWNED_NODE).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = post_edge(AZ_VIEW_NODE).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "already linked as references");
}

// ── POST /edges ontology rules (#224) ────────────────────────────────────────

#[tokio::test]
async fn contains_needs_edit_rights_on_the_child_too() {
    // §14: a home claims the child, so viewing it isn't enough. The pair
    // (an Article as a home) breaks the containment table as well: the 403
    // comes first, so the kinds stay hidden.
    let (status, _) = post_typed_edge(AZ_EDIT_NODE, AZ_VIEW_NODE, "contains").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn edge_rules_never_answer_a_stranger() {
    // No access to the target: the authZ answer, never the rule's 422.
    for edge_type in ["contains", "wiki_link", "depends_on"] {
        let (status, _) = post_typed_edge(AZ_EDIT_NODE, AZ_FOREIGN_NODE, edge_type).await;
        assert!(
            status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
            "{edge_type}: {status}"
        );
    }
}

#[tokio::test]
async fn a_home_can_be_released_from_the_childs_side() {
    let delete = |edge: u128| async move {
        claims_app("/edges", crate::routes::edges::router())
            .oneshot(
                Request::delete(format!("/edges/{}", Uuid::from_u128(edge)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    };
    // The caller edits the child (their Project) but not the home.
    assert_eq!(delete(AZ_FOREIGN_HOME_EDGE).await, StatusCode::NO_CONTENT);
    // `related_to` has no direction: either end's editor (§14, #226).
    assert_eq!(
        delete(AZ_FOREIGN_RELATED_EDGE).await,
        StatusCode::NO_CONTENT
    );
    // An editor of neither end (a stranger to one, a viewer of the other)
    // still may not.
    assert_eq!(
        delete(AZ_STRANGERS_RELATED_EDGE).await,
        StatusCode::FORBIDDEN
    );
    // Any other link still belongs to its source's editors.
    assert_eq!(delete(AZ_FOREIGN_LINK_EDGE).await, StatusCode::FORBIDDEN);
}

/// `POST /edges/{id}/convert` (#228): rights first (to delete the old link
/// and create the new one), then which conversions exist.
#[tokio::test]
async fn a_link_converts_only_for_someone_who_could_delete_and_recreate_it() {
    let convert = |edge: u128, to: &'static str| async move {
        claims_app("/edges", crate::routes::edges::router())
            .oneshot(
                Request::post(format!("/edges/{}/convert", Uuid::from_u128(edge)))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "edge_type": to }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    };
    // The child's editor may turn a home they can see into a related link,
    // which then runs from their own node (the stub asserts the swap).
    assert_eq!(
        convert(AZ_VISIBLE_HOME_EDGE, "related_to").await,
        StatusCode::NO_CONTENT
    );
    // ... but not into a references link, which belongs to the parent.
    assert_eq!(
        convert(AZ_VISIBLE_HOME_EDGE, "references").await,
        StatusCode::FORBIDDEN
    );
    // A related link needs a parent they can see.
    assert_eq!(
        convert(AZ_FOREIGN_HOME_EDGE, "related_to").await,
        StatusCode::FORBIDDEN
    );
    // Only after the rights: a structural target is no conversion.
    assert_eq!(
        convert(AZ_VISIBLE_HOME_EDGE, "depends_on").await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // A stranger is refused before anything is judged, and a directed link
    // belongs to its source's editors.
    assert_eq!(
        convert(AZ_STRANGERS_RELATED_EDGE, "wiki_link").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        convert(AZ_FOREIGN_LINK_EDGE, "related_to").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(convert(0xdead, "related_to").await, StatusCode::NOT_FOUND);
}

/// `GET /nodes/{id}/home` (#228): a home is named only to someone who can
/// see it; the child's editors learn a hidden one exists, nothing more.
#[tokio::test]
async fn a_hidden_home_is_shown_to_editors_only_and_never_named() {
    let home = |node: u128| async move {
        let resp = claims_app("/nodes", crate::routes::nodes::router())
            .oneshot(
                Request::get(format!("/nodes/{}/home", Uuid::from_u128(node)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default(),
        )
    };
    // The caller edits their Project: the hidden home exists, unnamed.
    let (status, body) = home(AZ_MY_PROJECT).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({ "home": {
            "edge_id": Uuid::from_u128(AZ_HIDDEN_HOME_EDGE), "node": null } })
    );
    // A viewer of the child isn't told.
    assert_eq!(
        home(AZ_VIEW_NODE).await.1,
        serde_json::json!({ "home": null })
    );
    // A home they can see is named.
    let (_, body) = home(AZ_EDIT_NODE).await;
    assert_eq!(
        body["home"]["node"]["id"],
        serde_json::json!(Uuid::from_u128(AZ_OWNED_NODE))
    );
    // A stranger gets nothing at all.
    assert_eq!(home(AZ_FOREIGN_NODE).await.0, StatusCode::FORBIDDEN);
}

/// `PUT /nodes/{id}/home` (#228): edit rights on the child, and on the new
/// home, before any rule; the stub panics if a refused call gets through.
#[tokio::test]
async fn setting_a_home_needs_both_ends_and_an_allowed_pair() {
    let put = |node: u128, parent: Option<u128>| async move {
        claims_app("/nodes", crate::routes::nodes::router())
            .oneshot(
                Request::put(format!("/nodes/{}/home", Uuid::from_u128(node)))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "parent_id": parent.map(Uuid::from_u128) }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    };
    // Only viewing the child: refused before anything else.
    assert_eq!(put(AZ_VIEW_NODE, None).await, StatusCode::FORBIDDEN);
    // A home they only view can't be claimed.
    assert_eq!(
        put(AZ_MY_PROJECT, Some(AZ_VIEW_NODE)).await,
        StatusCode::FORBIDDEN
    );
    // Rules after rights: itself, and an Article as a Project's home.
    assert_eq!(
        put(AZ_MY_PROJECT, Some(AZ_MY_PROJECT)).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        put(AZ_MY_PROJECT, Some(AZ_MY_ARTICLE)).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // The child's editor frees it, even from a home they can't see (§6.1).
    assert_eq!(put(AZ_MY_PROJECT, None).await, StatusCode::OK);
}

#[tokio::test]
async fn edge_kind_rules_refuse_with_the_reason() {
    let (status, body) = post_typed_edge(AZ_EDIT_NODE, AZ_OWNED_NODE, "wiki_link").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("wiki links come from"),
        "{body}"
    );
    let (status, body) = post_typed_edge(AZ_MY_PROJECT, AZ_MY_ARTICLE, "contains").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("a Project can't contain an Article"),
        "{body}"
    );
    // Allowed kinds reach the repo.
    let (status, _) = post_typed_edge(AZ_MY_ARTICLE, AZ_MY_PROJECT, "references").await;
    assert_eq!(status, StatusCode::CREATED);
}

// ── e2e test-identity header is inert outside e2e builds (#135) ──────────────

/// SECURITY: in a build without the `e2e-bypass` feature (every release
/// image) the `x-e2e-subject` header must not authenticate anyone: the
/// request still meets the real auth path (here: no OIDC configured → 500).
#[cfg(not(feature = "e2e-bypass"))]
#[tokio::test]
async fn e2e_subject_header_is_ignored_without_the_bypass_feature() {
    let resp = test_app()
        .oneshot(
            Request::get("/nodes")
                .header("x-e2e-subject", "other")
                .header("x-forwarded-for", "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Not authenticated: the same answer as with no header at all (the
    // detail "OIDC not configured" is logged, never sent).
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body_json(resp).await["error"], "internal error");
}

// ── GET /nodes/resolve-titles (#134) ─────────────────────────────────────────

async fn resolve(query: &str) -> (StatusCode, serde_json::Value) {
    let resp = claims_app("/nodes", crate::routes::nodes::router())
        .oneshot(
            Request::get(format!("/nodes/resolve-titles?titles={query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    (status, body_json(resp).await)
}

fn enc(json: &str) -> String {
    json.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn resolve_titles_lowercases_dedupes_and_omits_the_unmatched() {
    let (status, json) = resolve(&enc(r#"["Plan","plan","  ","Not Visible"]"#)).await;
    assert_eq!(status, StatusCode::OK);
    let obj = json.as_object().expect("a map");
    assert_eq!(obj.len(), 1, "{json}");
    assert!(obj.contains_key("plan"));
}

#[tokio::test]
async fn resolve_titles_validates_its_input() {
    let (status, _) = resolve(&enc("not json")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let many = format!(
        "[{}]",
        (0..101)
            .map(|i| format!("\"t{i}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let (status, _) = resolve(&enc(&many)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let long = format!("[\"{}\"]", "a".repeat(301));
    let (status, _) = resolve(&enc(&long)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, json) = resolve(&enc("[]")).await;
    assert_eq!((status, json), (StatusCode::OK, serde_json::json!({})));
}

// ── Project status (#143) ─────────────────────────────────────────────────────

async fn node_put(node: u128, body: serde_json::Value) -> StatusCode {
    claims_app("/nodes", crate::routes::nodes::router())
        .oneshot(
            Request::put(format!("/nodes/{}", Uuid::from_u128(node)))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn a_viewer_cannot_set_a_project_status() {
    let status = node_put(
        AZ_VIEW_NODE,
        serde_json::json!({ "project_status": "Done" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ── Save precondition (#161) ──────────────────────────────────────────────────

fn versioned_put() -> serde_json::Value {
    serde_json::json!({ "body": "x", "expected_updated_at": "2026-10-26T12:00:00Z" })
}

#[tokio::test]
async fn a_stale_save_version_is_a_412_for_an_editor() {
    assert_eq!(
        node_put(AZ_EDIT_NODE, versioned_put()).await,
        StatusCode::PRECONDITION_FAILED
    );
}

#[tokio::test]
async fn the_version_check_never_runs_for_a_viewer_or_a_stranger() {
    // Authorization comes first: a viewer or a stranger gets the same
    // answer as without a precondition, so the version check can't be used
    // to probe a node they may not edit (the stub 412s if it is reached).
    assert_eq!(
        node_put(AZ_VIEW_NODE, versioned_put()).await,
        StatusCode::FORBIDDEN
    );
    let stranger = node_put(AZ_FOREIGN_NODE, versioned_put()).await;
    assert!(
        stranger == StatusCode::FORBIDDEN || stranger == StatusCode::NOT_FOUND,
        "{stranger}"
    );
}

#[tokio::test]
async fn an_invalid_project_status_is_rejected_before_any_write() {
    // The stub's `update` is unimplemented: reaching it would panic, so a
    // 422 proves validation runs first.
    for bad in ["x".repeat(41), "two\nlines".to_string()] {
        let status = node_put(AZ_EDIT_NODE, serde_json::json!({ "project_status": bad })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[tokio::test]
async fn project_status_suggestions_are_the_callers_own() {
    let resp = claims_app("/nodes", crate::routes::nodes::router())
        .oneshot(
            Request::get("/nodes/project-statuses")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    // The stub echoes the owner it was asked for: the caller.
    assert_eq!(
        body,
        serde_json::json!([{ "status": "of test-sub", "count": 1 }])
    );
}

/// The dashboard asks for Areas in the caller's scope (#228): a non-admin
/// never gets the unfiltered admin scope.
#[tokio::test]
async fn the_dashboard_asks_for_areas_the_caller_can_see() {
    async fn area_title(claims: common::auth::AuthClaims) -> serde_json::Value {
        let resp = Router::new()
            .nest("/dashboard", crate::routes::tasks::dashboard_router())
            .layer(axum::Extension(claims))
            .with_state(test_state())
            .oneshot(Request::get("/dashboard").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        body[0]["area_title"].clone()
    }
    assert_eq!(
        area_title(fake_claims()).await,
        serde_json::json!("area for Some(\"test-sub\")")
    );
    assert_eq!(
        area_title(admin_claims()).await,
        serde_json::json!("area for None")
    );
}

/// A node favorite needs view rights (#228): its list shows the node's
/// live title. A node the caller can't see, or that doesn't exist, is a
/// 403 before anything is stored.
#[tokio::test]
async fn a_favorite_needs_view_rights_on_its_node() {
    async fn post(node: Option<u128>) -> StatusCode {
        let body = serde_json::json!({
            "node_id": node.map(az_node),
            "url": node.is_none().then_some("https://example.com"),
            "label": "x",
        });
        claims_app("/favorites", crate::routes::favorites::router())
            .oneshot(
                Request::post("/favorites")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }
    assert_eq!(post(Some(AZ_FOREIGN_NODE)).await, StatusCode::FORBIDDEN);
    assert_eq!(post(Some(0xdead_beef)).await, StatusCode::FORBIDDEN);
    assert_eq!(post(Some(AZ_VIEW_NODE)).await, StatusCode::CREATED);
    assert_eq!(post(None).await, StatusCode::CREATED, "a URL needs no node");
}

// ── Captures (#234) ───────────────────────────────────────────────────────────

async fn capture_call(
    claims: common::auth::AuthClaims,
    req: Request<Body>,
) -> (StatusCode, serde_json::Value) {
    let resp = Router::new()
        .nest("/captures", crate::routes::captures::router())
        .nest("/inbox", crate::routes::inbox::router())
        .nest("/notes", crate::routes::notes::note_router())
        .layer(axum::Extension(claims))
        .with_state(test_state())
        .oneshot(req)
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap_or_default())
}

fn json_req(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Captures are owner-only with no admin bypass (owner decision): the
/// routes always ask for the caller's own, so an admin reaches only theirs
/// and gets 404 for anyone else's.
#[tokio::test]
async fn an_admin_cannot_reach_someone_elses_capture() {
    let others = Uuid::from_u128(AZ_OTHERS_CAPTURE);
    let (status, list) = capture_call(
        admin_claims(),
        Request::get("/captures").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["owner_id"], serde_json::json!(admin_claims().sub));
    for req in [
        json_req(
            "PATCH",
            &format!("/captures/{others}"),
            serde_json::json!({ "body": "x" }),
        ),
        Request::delete(format!("/captures/{others}"))
            .body(Body::empty())
            .unwrap(),
        Request::post(format!("/captures/{others}/restore"))
            .body(Body::empty())
            .unwrap(),
    ] {
        assert_eq!(
            capture_call(admin_claims(), req).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn quick_capture_lands_a_capture_for_the_caller() {
    let (status, body) = capture_call(
        fake_claims(),
        json_req(
            "POST",
            "/inbox/quick",
            serde_json::json!({ "title": "an idea", "priority": "high" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["id"].is_string());
    assert_eq!(body["truncated"], serde_json::json!(false));
    // Nothing to capture.
    let (status, _) = capture_call(
        fake_claims(),
        json_req("POST", "/inbox/quick", serde_json::json!({ "title": "  " })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = capture_call(
        fake_claims(),
        json_req("POST", "/captures", serde_json::json!({ "body": " \n " })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

fn convert_req(capture: Uuid, body: serde_json::Value) -> Request<Body> {
    json_req("POST", &format!("/captures/{capture}/convert"), body)
}

/// A capture becomes exactly one thing (#234, §9.1), in the caller's name.
#[tokio::test]
async fn a_capture_converts_into_a_node_a_note_or_a_task() {
    let mine = Uuid::new_v4();
    for (body, into) in [
        (
            serde_json::json!({ "into": "node", "node_type": "project", "title": " Plan " }),
            "node",
        ),
        (
            serde_json::json!({
                "into": "node", "node_type": "project", "title": "Sub",
                "home": Uuid::from_u128(AZ_MY_PROJECT),
            }),
            "node",
        ),
        (
            serde_json::json!({
                "into": "note", "node_id": Uuid::from_u128(AZ_EDIT_NODE), "body": "met Ana",
            }),
            "note",
        ),
        (
            serde_json::json!({
                "into": "task", "node_id": Uuid::from_u128(AZ_MY_PROJECT),
                "title": "call the bank", "due_date": "2026-11-07",
            }),
            "task",
        ),
    ] {
        let (status, out) = capture_call(fake_claims(), convert_req(mine, body)).await;
        assert_eq!(status, StatusCode::CREATED, "{into}: {out}");
        assert_eq!(out["into"], into);
    }
}

/// Someone else's capture is a 404, an admin's request included, even
/// with a destination the admin may write: never a hint it exists.
#[tokio::test]
async fn nobody_converts_someone_elses_capture() {
    let others = Uuid::from_u128(AZ_OTHERS_CAPTURE);
    let note = serde_json::json!({
        "into": "note", "node_id": Uuid::from_u128(AZ_OWNED_NODE), "body": "x",
    });
    for claims in [fake_claims(), admin_claims()] {
        let (status, _) = capture_call(claims, convert_req(others, note.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

/// The destination's rules come before anything is made: editor rights
/// (403), then the kinds and the text (422). The stub repo panics if a
/// refused destination reaches it.
#[tokio::test]
async fn a_conversion_is_refused_before_anything_is_made() {
    let mine = Uuid::new_v4();
    let view = Uuid::from_u128(AZ_VIEW_NODE);
    let project = Uuid::from_u128(AZ_MY_PROJECT);
    let article = Uuid::from_u128(AZ_MY_ARTICLE);
    for (body, want) in [
        (
            serde_json::json!({ "into": "note", "node_id": view, "body": "x" }),
            StatusCode::FORBIDDEN,
        ),
        (
            serde_json::json!({ "into": "task", "node_id": view, "title": "x" }),
            StatusCode::FORBIDDEN,
        ),
        (
            serde_json::json!({
                "into": "node", "node_type": "article", "title": "x", "home": view,
            }),
            StatusCode::FORBIDDEN,
        ),
        // A Task needs a Project (§9.3).
        (
            serde_json::json!({ "into": "task", "node_id": article, "title": "x" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        // A Project holds only Projects (§3.3).
        (
            serde_json::json!({
                "into": "node", "node_type": "article", "title": "x", "home": project,
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        // A Task's title must fit (owner decision): reworded, never cut.
        (
            serde_json::json!({
                "into": "task", "node_id": project, "title": "x".repeat(501),
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            serde_json::json!({ "into": "note", "node_id": project, "body": "  " }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        // A node keeps create_node's limit (500 bytes): 300 Cyrillic
        // letters fit a Task, not a node the editor could save again.
        (
            serde_json::json!({
                "into": "node", "node_type": "article", "title": "ж".repeat(300),
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, out) = capture_call(fake_claims(), convert_req(mine, body.clone())).await;
        assert_eq!(status, want, "{body}: {out}");
    }
}

/// Every note has a node (§9.2): the Notes page can't make one without.
#[tokio::test]
async fn a_note_needs_a_node() {
    let (status, _) = capture_call(
        fake_claims(),
        json_req("POST", "/notes", serde_json::json!({ "body": "loose" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // With a node, its editor rights come first.
    let (status, _) = capture_call(
        fake_claims(),
        json_req(
            "POST",
            "/notes",
            serde_json::json!({ "body": "on it", "node_id": Uuid::from_u128(AZ_VIEW_NODE) }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ── People (#147) ─────────────────────────────────────────────────────────────

fn people_app() -> Router {
    Router::new()
        .nest("/people", crate::routes::people::router())
        .nest(
            "/nodes/{node_id}/people",
            crate::routes::people::node_router(),
        )
        .layer(axum::Extension(fake_claims()))
        .with_state(test_state())
}

async fn people_call(
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = people_app()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn people_routes_registered() {
    assert_route_registered("GET", "/people").await;
    assert_route_registered("GET", "/people/roles").await;
    assert_route_registered("GET", &format!("/nodes/{}/people", Uuid::new_v4())).await;
}

#[tokio::test]
async fn someone_elses_person_is_a_404_like_a_missing_one() {
    let (status, _) = people_call(
        "GET",
        &format!("/people/{}", Uuid::new_v4()),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = people_call(
        "GET",
        &format!("/people/{}", Uuid::from_u128(AZ_PERSON)),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(
        (status, body["display_name"].as_str()),
        (StatusCode::OK, Some("Ada"))
    );
}

#[tokio::test]
async fn an_invalid_person_is_rejected_before_any_write() {
    // The stub's `create` is unimplemented: reaching it would panic.
    for bad in [
        serde_json::json!({ "display_name": "   " }),
        serde_json::json!({ "display_name": "Ada", "emails": [{ "value": "not-an-email" }] }),
    ] {
        let (status, _) = people_call("POST", "/people", bad).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[tokio::test]
async fn a_matching_email_is_a_duplicate_warning_not_a_silent_create() {
    let (status, body) = people_call(
        "POST",
        "/people",
        serde_json::json!({ "display_name": "Ada L.", "emails": [{ "value": "Taken@Example.com" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["matches"][0]["display_name"], "Ada");
}

#[tokio::test]
async fn adding_a_taken_email_on_edit_warns_before_any_write() {
    // Someone else in the registry has it: a warning before any write.
    let (status, body) = people_call(
        "PUT",
        &format!("/people/{}", Uuid::new_v4()),
        serde_json::json!({
            "display_name": "Ada L.",
            "emails": [{ "value": "taken@example.com" }],
            "addresses": [],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["matches"][0]["display_name"], "Ada");
}

#[tokio::test]
async fn a_pre_155_edit_reads_the_stored_addresses_owner_scoped() {
    // A stale tab's body has no `addresses`, so the route reads the stored
    // list first. That read is owner-scoped: another owner's person (or a
    // missing one) is a 404, before the duplicate check or any write.
    let (status, _) = people_call(
        "PUT",
        &format!("/people/{}", Uuid::new_v4()),
        serde_json::json!({ "display_name": "Ada L.", "address": "1 Main St" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_address_entry_is_validated_like_emails_and_phones() {
    let (status, _) = people_call(
        "POST",
        "/people",
        serde_json::json!({
            "display_name": "Ada",
            "addresses": [
                { "label": "home", "value": "1 Main St", "preferred": true },
                { "label": "office", "value": "2 Elm St", "preferred": true },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_node_viewer_can_list_its_people_and_a_stranger_cannot() {
    let (status, _) = people_call(
        "GET",
        &format!("/nodes/{}/people", Uuid::from_u128(AZ_VIEW_NODE)),
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = people_call(
        "GET",
        &format!("/nodes/{}/people", Uuid::from_u128(AZ_FOREIGN_NODE)),
        serde_json::json!({}),
    )
    .await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "{status}"
    );
}

#[tokio::test]
async fn only_the_nodes_owner_can_change_its_people() {
    let body = serde_json::json!({ "person_id": Uuid::from_u128(AZ_PERSON) });
    // A viewer, an editor who isn't the owner, and a stranger: all refused
    // before anything is written (the stub's add_membership would panic).
    for node in [AZ_VIEW_NODE, AZ_EDIT_NODE, AZ_FOREIGN_NODE, AZ_OWNED_NODE] {
        let (status, _) = people_call(
            "POST",
            &format!("/nodes/{}/people", Uuid::from_u128(node)),
            body.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "node {node:#x}");
    }
}

#[tokio::test]
async fn a_role_and_note_on_attach_are_validated_and_still_owner_only() {
    // #162 sends Role and Note with the attach. A bad one is refused before
    // any write (the stub's add_membership would panic)...
    let person = Uuid::from_u128(AZ_PERSON);
    for bad in [
        serde_json::json!({ "person_id": person, "role": "lead\nreviewer" }),
        serde_json::json!({ "person_id": person, "role": "x".repeat(101) }),
        serde_json::json!({ "person_id": person, "note": "n".repeat(2001) }),
    ] {
        let (status, _) = people_call(
            "POST",
            &format!("/nodes/{}/people", Uuid::from_u128(AZ_OWNED_NODE)),
            bad,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    // ...and a valid one doesn't widen who may attach: a viewer, an editor
    // who isn't the canonical owner, and a stranger are all still refused.
    let body =
        serde_json::json!({ "person_id": person, "role": "Sponsor", "note": "met at kickoff" });
    for node in [AZ_VIEW_NODE, AZ_EDIT_NODE, AZ_FOREIGN_NODE, AZ_OWNED_NODE] {
        let (status, _) = people_call(
            "POST",
            &format!("/nodes/{}/people", Uuid::from_u128(node)),
            body.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "node {node:#x}");
    }
}

/// Records every activity write, so a test can assert what a handler logged.
#[derive(Default)]
struct RecordingActivityRepo(std::sync::Mutex<Vec<ActivityAction>>);
#[async_trait]
impl ActivityRepo for RecordingActivityRepo {
    async fn record(
        &self,
        _: NodeId,
        _: &str,
        action: ActivityAction,
        _: serde_json::Value,
    ) -> Result<(), EmberTroveError> {
        self.0.lock().expect("lock").push(action);
        Ok(())
    }
    async fn list(&self, _: NodeId, _: i64) -> Result<Vec<ActivityEntry>, EmberTroveError> {
        Ok(vec![])
    }
    async fn list_recent_for_owner(
        &self,
        _: &str,
        _: chrono::DateTime<chrono::Utc>,
        _: i64,
    ) -> Result<Vec<common::activity::RecentActivityEntry>, EmberTroveError> {
        Ok(vec![])
    }
    async fn list_all(&self) -> Result<Vec<common::activity::ActivityRecord>, EmberTroveError> {
        Ok(vec![])
    }
    async fn latest_of(
        &self,
        _: NodeId,
        _: ActivityAction,
    ) -> Result<Option<ActivityEntry>, EmberTroveError> {
        Ok(None)
    }
}

#[tokio::test]
async fn deleting_a_node_writes_no_activity_entry() {
    // Regression (#208): the handler logged "deleted" after the delete, when
    // the per-node log (FK, ON DELETE CASCADE) could no longer take the row,
    // so every delete failed that write and logged an FK warning.
    let activity = Arc::new(RecordingActivityRepo::default());
    let mut state = test_state();
    state.activity = activity.clone();
    let app = Router::new()
        .nest("/nodes", crate::routes::nodes::router())
        .layer(axum::Extension(fake_claims()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::delete(format!("/nodes/{}", Uuid::from_u128(AZ_OWNED_NODE)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(
        activity.0.lock().expect("lock").is_empty(),
        "no activity write after the node is gone"
    );
}

#[tokio::test]
async fn the_data_health_report_is_the_callers_own_even_for_an_admin() {
    // §14: an admin's report is their own data like anyone's. The route
    // must pass `claims.sub`, never the admin's "everything" scope.
    let repo = Arc::new(StubDataHealthRepo::default());
    let mut state = test_state();
    state.data_health = repo.clone();
    let app = Router::new()
        .nest("/data-health", crate::routes::data_health::router())
        .layer(axum::Extension(admin_claims()))
        .with_state(state);

    let resp = app
        .oneshot(Request::get("/data-health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["rules"].is_array());
    assert_eq!(*repo.0.lock().expect("lock"), vec![admin_claims().sub]);
}

#[tokio::test]
async fn admin_data_health_counts_are_admin_only_and_carry_no_content() {
    let app = |claims: common::auth::AuthClaims| {
        Router::new()
            .nest("/admin", crate::routes::admin::router())
            .layer(axum::Extension(claims))
            .with_state(test_state())
    };
    let denied = app(fake_claims())
        .oneshot(
            Request::get("/admin/data-health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let ok = app(admin_claims())
        .oneshot(
            Request::get("/admin/data-health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let body = body_json(ok).await;
    assert_eq!(
        body,
        serde_json::json!([{ "rule": "orphan_task", "count": 3 }])
    );
}

// ── §3.2 capability gates (#216) ──────────────────────────────────────────────

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::post(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn status_and_error(resp: axum::response::Response) -> (StatusCode, String) {
    let status = resp.status();
    let body = body_json(resp).await;
    (
        status,
        body["error"].as_str().unwrap_or_default().to_string(),
    )
}

async fn node_task_post(node: u128, claims: common::auth::AuthClaims) -> (StatusCode, String) {
    let app = Router::new()
        .nest(
            "/nodes/{node_id}/tasks",
            crate::routes::tasks::node_task_router(),
        )
        .layer(axum::Extension(claims))
        .with_state(test_state());
    let resp = app
        .oneshot(post_json(
            &format!("/nodes/{}/tasks", Uuid::from_u128(node)),
            serde_json::json!({ "title": "t" }),
        ))
        .await
        .unwrap();
    status_and_error(resp).await
}

#[tokio::test]
async fn a_task_can_be_added_only_to_a_project() {
    let (ok, _) = node_task_post(AZ_MY_PROJECT, fake_claims()).await;
    assert_eq!(ok, StatusCode::CREATED);
    let (refused, why) = node_task_post(AZ_MY_ARTICLE, fake_claims()).await;
    assert_eq!(refused, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(why, "only Projects have tasks");
}

#[tokio::test]
async fn the_task_gate_never_tells_a_stranger_a_nodes_kind() {
    // Authorization runs first: a viewer and a stranger get 403, never 422.
    assert_eq!(
        node_task_post(AZ_VIEW_NODE, fake_claims()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        node_task_post(AZ_FOREIGN_NODE, fake_claims()).await.0,
        StatusCode::FORBIDDEN
    );
    // A missing node: 403 for anyone without a permission row, 404 for an
    // admin (who passes authorization and used to reach the FK insert).
    assert_eq!(
        node_task_post(0xdead, fake_claims()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        node_task_post(0xdead, admin_claims()).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_standalone_task_can_name_only_a_project() {
    let post = |node: Option<u128>| async move {
        let mut body = serde_json::json!({ "title": "t" });
        if let Some(n) = node {
            body["node_id"] = serde_json::json!(Uuid::from_u128(n));
        }
        let resp = claims_app("/tasks", crate::routes::tasks::task_router())
            .oneshot(post_json("/tasks", body))
            .await
            .unwrap();
        resp.status()
    };
    assert_eq!(
        post(None).await,
        StatusCode::CREATED,
        "no node: still allowed until S5c"
    );
    assert_eq!(post(Some(AZ_MY_PROJECT)).await, StatusCode::CREATED);
    assert_eq!(
        post(Some(AZ_MY_ARTICLE)).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn a_task_already_on_a_non_project_stays_editable() {
    // AZ_EDIT_TASK sits on an Article: editing it isn't a move, so §3.2
    // doesn't block it (it's listed on Data health instead).
    let status = task_patch(AZ_EDIT_TASK, serde_json::json!({ "title": "renamed" })).await;
    assert_eq!(status, StatusCode::OK);
    let same_node = Uuid::from_u128(AZ_EDIT_NODE);
    let status = task_patch(AZ_EDIT_TASK, serde_json::json!({ "node_id": same_node })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "naming its own node again isn't a move"
    );
}

async fn member_post(node: u128) -> (StatusCode, String) {
    let resp = people_app()
        .oneshot(post_json(
            &format!("/nodes/{}/people", Uuid::from_u128(node)),
            serde_json::json!({ "person_id": Uuid::from_u128(AZ_PERSON) }),
        ))
        .await
        .unwrap();
    status_and_error(resp).await
}

#[tokio::test]
async fn a_person_can_be_added_only_to_a_project() {
    let (refused, why) = member_post(AZ_MY_ARTICLE).await;
    assert_eq!(refused, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(why, "only Projects have people");
    // On the Project the membership is written (the stub panics on any
    // other node) and read back.
    assert_eq!(member_post(AZ_MY_PROJECT).await.0, StatusCode::CREATED);
    // Not the node's owner: 403 before any kind check.
    assert_eq!(member_post(AZ_EDIT_NODE).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_project_status_can_be_set_only_on_a_project_but_always_cleared() {
    let stale = chrono::Utc::now().to_rfc3339();
    // A precondition makes the stub answer 412, which proves the request got
    // past the gate without needing a real update.
    let set = |node: u128, value: serde_json::Value| {
        let stale = stale.clone();
        async move {
            node_put(
                node,
                serde_json::json!({ "project_status": value, "expected_updated_at": stale }),
            )
            .await
        }
    };
    assert_eq!(
        set(AZ_MY_ARTICLE, "Active".into()).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        set(AZ_MY_PROJECT, "Active".into()).await,
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        set(AZ_MY_ARTICLE, serde_json::Value::Null).await,
        StatusCode::PRECONDITION_FAILED,
        "clearing is the Data health fix: always allowed"
    );
    assert_eq!(
        set(AZ_MY_ARTICLE, "  ".into()).await,
        StatusCode::PRECONDITION_FAILED,
        "blank is a clear"
    );
    assert_eq!(
        set(AZ_VIEW_NODE, "Active".into()).await,
        StatusCode::FORBIDDEN
    );
}

// ── Account lifecycle (#239) ─────────────────────────────────────────────────

const ADMIN_SUB: &str = "0a0a0a0a-0000-4000-8000-00000000000a";
const U1: &str = "00000000-0000-4000-8000-000000000001";
const U2: &str = "00000000-0000-4000-8000-000000000002";

/// An admin whose `sub` is a real account id, as Cognito issues them.
fn uuid_admin() -> common::auth::AuthClaims {
    common::auth::AuthClaims {
        sub: ADMIN_SUB.to_string(),
        ..admin_claims()
    }
}

/// One log shared by the account repo and the directory fake, so a test can
/// assert the order of the steps across both.
type StepLog = Arc<std::sync::Mutex<Vec<String>>>;

fn steps(log: &StepLog) -> Vec<String> {
    log.lock().unwrap().clone()
}

#[derive(Default)]
struct StubAccountRepo {
    rows: std::sync::Mutex<std::collections::HashSet<String>>,
    log: StepLog,
}

#[async_trait]
impl crate::repo::account::AccountRepo for StubAccountRepo {
    async fn suspended(&self) -> Result<std::collections::HashSet<String>, EmberTroveError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn suspend(&self, subject_id: &str, _by: &str) -> Result<(), EmberTroveError> {
        self.log.lock().unwrap().push(format!("row+ {subject_id}"));
        self.rows.lock().unwrap().insert(subject_id.to_string());
        Ok(())
    }
    async fn unsuspend(&self, subject_id: &str) -> Result<(), EmberTroveError> {
        self.log.lock().unwrap().push(format!("row- {subject_id}"));
        self.rows.lock().unwrap().remove(subject_id);
        Ok(())
    }
    async fn mark_deleted(&self, subject_id: &str, _by: &str) -> Result<(), EmberTroveError> {
        self.log
            .lock()
            .unwrap()
            .push(format!("row deleted {subject_id}"));
        self.rows.lock().unwrap().insert(subject_id.to_string());
        Ok(())
    }
    async fn purge_deleted_before(
        &self,
        _: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, EmberTroveError> {
        Ok(0)
    }
}

/// A Cognito stand-in that records every call. `fail` names an operation
/// that errors; `missing` makes every per-user call answer "not found".
#[derive(Default)]
struct FakeDirectory {
    log: StepLog,
    fail: Option<&'static str>,
    missing: bool,
}

impl FakeDirectory {
    fn call(&self, op: &str, user: &str) -> Result<(), crate::error::ApiError> {
        self.log.lock().unwrap().push(format!("{op} {user}"));
        if self.missing {
            return Err(crate::error::ApiError::NotFound("user not found".into()));
        }
        if self.fail == Some(op) {
            return Err(crate::error::ApiError::Internal(
                "identity provider operation failed".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl crate::admin::UserDirectory for FakeDirectory {
    async fn list_users(&self) -> Result<Vec<common::admin::AdminUser>, crate::error::ApiError> {
        Ok([ADMIN_SUB, U1, U2]
            .into_iter()
            .map(|id| common::admin::AdminUser {
                id: id.to_string(),
                username: format!("{id}@example.com"),
                email: Some(format!("{id}@example.com")),
                first_name: None,
                last_name: None,
                enabled: true,
                realm_roles: vec![],
                suspended: false,
            })
            .collect())
    }
    async fn create_user(
        &self,
        _: &common::admin::CreateAdminUserRequest,
    ) -> Result<common::admin::AdminUser, crate::error::ApiError> {
        unimplemented!()
    }
    async fn find_user_by_email(
        &self,
        _: &str,
    ) -> Result<Option<common::admin::AdminUser>, crate::error::ApiError> {
        Ok(None)
    }
    async fn delete_user(&self, user: &str) -> Result<(), crate::error::ApiError> {
        self.call("delete", user)
    }
    async fn list_groups(&self) -> Result<Vec<String>, crate::error::ApiError> {
        Ok(vec!["admin".into()])
    }
    async fn set_user_groups(
        &self,
        user: &str,
        _: &[String],
    ) -> Result<(), crate::error::ApiError> {
        self.call("groups", user)
    }
    async fn disable_user(&self, user: &str) -> Result<(), crate::error::ApiError> {
        self.call("disable", user)
    }
    async fn enable_user(&self, user: &str) -> Result<(), crate::error::ApiError> {
        self.call("enable", user)
    }
    async fn sign_out_everywhere(&self, user: &str) -> Result<(), crate::error::ApiError> {
        self.call("sign-out", user)
    }
}

/// A state whose account repo and directory share one step log.
fn lifecycle_state(mut directory: FakeDirectory) -> (AppState, StepLog) {
    let log = StepLog::default();
    directory.log = log.clone();
    let mut state = test_state();
    state.accounts = Arc::new(StubAccountRepo {
        log: log.clone(),
        ..StubAccountRepo::default()
    });
    state.cognito_admin = Some(Arc::new(directory));
    (state, log)
}

fn admin_app_with(state: AppState, claims: common::auth::AuthClaims) -> Router {
    Router::new()
        .nest("/admin", crate::routes::admin::router())
        .layer(axum::Extension(claims))
        .with_state(state)
}

async fn admin_call(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn account_lifecycle_routes_are_admin_only() {
    let (state, log) = lifecycle_state(FakeDirectory::default());
    let app = admin_app_with(state.clone(), fake_claims());
    for (method, uri) in [
        ("POST", format!("/admin/users/{U1}/suspend")),
        ("POST", format!("/admin/users/{U1}/reactivate")),
        ("DELETE", format!("/admin/users/{U1}")),
        ("PUT", format!("/admin/users/{U1}/roles")),
    ] {
        let (status, _) = admin_call(&app, method, &uri, r#"{"roles":[]}"#).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }
    assert!(steps(&log).is_empty(), "nothing may run for a non-admin");
    assert!(!state.suspended.contains(U1).await);
}

/// SECURITY (#239): the last admin can't remove the only way back in.
#[tokio::test]
async fn an_admin_cannot_suspend_delete_or_demote_themselves() {
    let (state, log) = lifecycle_state(FakeDirectory::default());
    let app = admin_app_with(state.clone(), uuid_admin());
    let me = ADMIN_SUB;
    for (method, uri, body) in [
        ("POST", format!("/admin/users/{me}/suspend"), "{}"),
        ("DELETE", format!("/admin/users/{me}"), "{}"),
        (
            "PUT",
            format!("/admin/users/{me}/roles"),
            r#"{"roles":["user"]}"#,
        ),
    ] {
        let (status, _) = admin_call(&app, method, &uri, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{method} {uri}");
    }
    assert!(steps(&log).is_empty());
    assert!(!state.suspended.contains(me).await);

    // Changing your own roles while keeping admin is fine.
    let (status, _) = admin_call(
        &app,
        "PUT",
        &format!("/admin/users/{me}/roles"),
        r#"{"roles":["admin","user"]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn suspend_blocks_the_api_before_asking_cognito() {
    let (state, log) = lifecycle_state(FakeDirectory::default());
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "POST", &format!("/admin/users/{U1}/suspend"), "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        steps(&log),
        [
            format!("row+ {U1}"),
            format!("disable {U1}"),
            format!("sign-out {U1}")
        ],
        "the row lands before Cognito is asked"
    );
    assert!(state.suspended.contains(U1).await);

    let (status, body) = admin_call(&app, "GET", "/admin/users", "").await;
    assert_eq!(status, StatusCode::OK);
    let users: Vec<common::admin::AdminUser> = serde_json::from_str(&body).unwrap();
    let flags: Vec<(String, bool)> = users.into_iter().map(|u| (u.id, u.suspended)).collect();
    assert_eq!(
        flags,
        [
            (ADMIN_SUB.to_string(), false),
            (U1.to_string(), true),
            (U2.to_string(), false)
        ]
    );
}

/// Fails closed: a Cognito failure leaves the account refused by the API,
/// and the admin is told it didn't fully land.
#[tokio::test]
async fn a_cognito_failure_leaves_the_account_suspended() {
    for op in ["disable", "sign-out"] {
        let (state, _) = lifecycle_state(FakeDirectory {
            fail: Some(op),
            ..FakeDirectory::default()
        });
        let app = admin_app_with(state.clone(), uuid_admin());
        let (status, body) =
            admin_call(&app, "POST", &format!("/admin/users/{U1}/suspend"), "").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{op}");
        assert!(body.contains("suspended in Ember Trove"), "{body}");
        assert!(state.suspended.contains(U1).await, "{op}");
    }
}

#[tokio::test]
async fn suspending_an_account_cognito_lacks_keeps_it_refused() {
    let (state, _) = lifecycle_state(FakeDirectory {
        missing: true,
        ..FakeDirectory::default()
    });
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "POST", &format!("/admin/users/{U1}/suspend"), "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(state.suspended.contains(U1).await);
}

#[tokio::test]
async fn reactivate_unblocks_only_after_cognito() {
    let (state, log) = lifecycle_state(FakeDirectory::default());
    state.accounts.suspend(U1, "admin").await.unwrap();
    state.suspended.insert(U1).await;
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "POST", &format!("/admin/users/{U1}/reactivate"), "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        steps(&log),
        [
            format!("row+ {U1}"),
            format!("enable {U1}"),
            format!("row- {U1}")
        ]
    );
    assert!(!state.suspended.contains(U1).await);
}

#[tokio::test]
async fn a_failed_reactivation_stays_suspended() {
    for directory in [
        FakeDirectory {
            fail: Some("enable"),
            ..FakeDirectory::default()
        },
        FakeDirectory {
            missing: true,
            ..FakeDirectory::default()
        },
    ] {
        let (state, log) = lifecycle_state(directory);
        state.suspended.insert(U1).await;
        let app = admin_app_with(state.clone(), uuid_admin());
        let (status, _) =
            admin_call(&app, "POST", &format!("/admin/users/{U1}/reactivate"), "").await;
        assert!(status.is_client_error() || status.is_server_error());
        assert!(state.suspended.contains(U1).await);
        assert!(!steps(&log).iter().any(|s| s.starts_with("row-")));
    }
}

/// A router with the real `reject_suspended` layer, as `build_router`
/// stacks it inside `require_auth`, and `claims` standing in for a valid
/// token.
fn session_app(state: AppState, claims: common::auth::AuthClaims) -> Router {
    Router::new()
        .merge(crate::routes::auth::protected_router())
        .nest("/nodes", crate::routes::nodes::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::suspended::reject_suspended,
        ))
        .layer(axum::Extension(claims))
        .with_state(state)
}

/// SECURITY (#239): a token issued before the suspension stops working at
/// once, with the same answer as a bad token.
#[tokio::test]
async fn a_suspended_account_s_valid_token_gets_401() {
    let state = test_state();
    let app = session_app(state.clone(), fake_claims());
    for uri in ["/auth/me", "/nodes"] {
        let resp = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{uri} before suspension");
    }

    state.suspended.insert(&fake_claims().sub).await;
    for uri in ["/auth/me", "/nodes"] {
        let resp = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(body_json(resp).await["error"], "invalid token");
    }

    // Someone else's session is untouched.
    let other = common::auth::AuthClaims {
        sub: "someone-else".into(),
        ..fake_claims()
    };
    let resp = session_app(state, other)
        .oneshot(Request::get("/auth/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// SECURITY (#239): Cognito accepts an email alias, or the UUID in another
/// case, as a username. Such an id would miss the self-guard and the
/// suspended list, so only the canonical id is accepted.
#[tokio::test]
async fn only_the_canonical_account_id_is_accepted() {
    let (state, log) = lifecycle_state(FakeDirectory::default());
    let app = admin_app_with(state.clone(), uuid_admin());
    let upper = ADMIN_SUB.to_uppercase();
    for id in [
        upper.as_str(),
        "admin@example.com",
        "0a0a0a0a00004000800000000000000a",
        "{0a0a0a0a-0000-4000-8000-00000000000a}",
        "not-an-id",
    ] {
        for (method, uri, body) in [
            ("POST", format!("/admin/users/{id}/suspend"), ""),
            ("POST", format!("/admin/users/{id}/reactivate"), ""),
            ("DELETE", format!("/admin/users/{id}"), ""),
            ("PUT", format!("/admin/users/{id}/roles"), r#"{"roles":[]}"#),
        ] {
            let (status, _) = admin_call(&app, method, &uri, body).await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{method} {uri}");
        }
    }
    assert!(
        steps(&log).is_empty(),
        "nothing may run for a non-canonical id"
    );
}

#[tokio::test]
async fn delete_locks_the_account_out_first_and_keeps_it_refused() {
    let (state, log) = lifecycle_state(FakeDirectory::default());
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "DELETE", &format!("/admin/users/{U1}"), "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        steps(&log),
        [
            format!("row+ {U1}"),
            format!("delete {U1}"),
            format!("row deleted {U1}")
        ]
    );
    // An ID token issued before the deletion is still refused.
    assert!(state.suspended.contains(U1).await);
}

/// A retry after Cognito already deleted the account still finishes.
#[tokio::test]
async fn deleting_an_account_cognito_lacks_still_finishes() {
    let (state, log) = lifecycle_state(FakeDirectory {
        missing: true,
        ..FakeDirectory::default()
    });
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "DELETE", &format!("/admin/users/{U1}"), "").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(steps(&log).contains(&format!("row deleted {U1}")));
    assert!(state.suspended.contains(U1).await);
}

#[tokio::test]
async fn a_failed_cognito_delete_leaves_the_account_suspended() {
    let (state, log) = lifecycle_state(FakeDirectory {
        fail: Some("delete"),
        ..FakeDirectory::default()
    });
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "DELETE", &format!("/admin/users/{U1}"), "").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(state.suspended.contains(U1).await);
    assert!(!steps(&log).iter().any(|s| s.starts_with("row deleted")));
}

/// Without a configured directory nothing is written: the route can't
/// finish, so it mustn't half-start.
#[tokio::test]
async fn suspend_without_a_directory_writes_nothing() {
    let state = test_state();
    let app = admin_app_with(state.clone(), uuid_admin());
    let (status, _) = admin_call(&app, "POST", &format!("/admin/users/{U1}/suspend"), "").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(state.accounts.suspended().await.unwrap().is_empty());
    assert!(!state.suspended.contains(U1).await);
}

/// The suspended check fails closed: with no claims in place (the layers
/// stacked the wrong way round) every request is refused.
#[tokio::test]
async fn reject_suspended_refuses_a_request_without_claims() {
    let state = test_state();
    let app = Router::new()
        .merge(crate::routes::auth::protected_router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::suspended::reject_suspended,
        ))
        .with_state(state);
    let resp = app
        .oneshot(Request::get("/auth/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
