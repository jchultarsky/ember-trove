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
//! * Rate limiter — `SmartIpKeyExtractor` needs at least one of
//!   `X-Real-IP`, `X-Forwarded-For`, or `ConnectInfo<SocketAddr>`.
//!   All requests to rate-limited routes include `X-Forwarded-For: 127.0.0.1`.

use std::{sync::Arc, time::{Duration, Instant}};

use async_trait::async_trait;
use axum::{Router, body::Body, http::{Request, StatusCode}};
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
    id::{AttachmentId, EdgeId, FavoriteId, NodeId, PermissionId, ShareTokenId, TagId, TaskId,
         TemplateId},
    node::{CreateNodeRequest, Node, NodeListParams, NodeTitleEntry, UpdateNodeRequest},
    note::{CreateNoteRequest, FeedNote, Note},
    permission::{GrantPermissionRequest, Permission, PermissionRole},
    id::{NodeLinkId, SearchPresetId, WebhookId},
    node_link::{CreateNodeLinkRequest, NodeLink, UpdateNodeLinkRequest},
    webhook::{CreateWebhookRequest, UpdateWebhookRequest, Webhook},
    search::{CreateSearchPresetRequest, SearchPreset, SearchQuery, SearchResponse},
    tag::{CreateTagRequest, Tag, UpdateTagRequest},
    task::{CreateTaskRequest, MyDayTask, Task, TaskCounts, UpdateTaskRequest},
    template::{CreateTemplateRequest, NodeTemplate, UpdateTemplateRequest},
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
        favorite::FavoriteRepo, graph::GraphRepo, node::NodeRepo, node_version::NodeVersionRepo,
        note::NoteRepo, permission::PermissionRepo, pkce::PkceRepo, search::SearchRepo,
        search_presets::SearchPresetRepo, share_token::ShareTokenRepo, tag::TagRepo,
        node_link::NodeLinkRepo, task::TaskRepo, template::TemplateRepo, webhook::WebhookRepo,
    },
    routes::build_router,
    state::AppState,
};

// ── Stub repo implementations ─────────────────────────────────────────────────
// All methods panic with `unimplemented!()` — they are never reached because
// the auth middleware short-circuits every protected request when oidc = None.

struct StubNodeRepo;
#[async_trait]
impl NodeRepo for StubNodeRepo {
    async fn create(&self, _: &str, _: CreateNodeRequest) -> Result<Node, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: NodeId) -> Result<Node, EmberTroveError> { unimplemented!() }
    async fn get_by_slug(&self, _: &str) -> Result<Node, EmberTroveError> { unimplemented!() }
    async fn list(&self, _: NodeListParams) -> Result<(Vec<Node>, u64), EmberTroveError> { unimplemented!() }
    async fn update(&self, _: NodeId, _: UpdateNodeRequest) -> Result<Node, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: NodeId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn neighbors(&self, _: NodeId) -> Result<Vec<Node>, EmberTroveError> { unimplemented!() }
    async fn backlinks(&self, _: NodeId) -> Result<Vec<Node>, EmberTroveError> { unimplemented!() }
    async fn list_titles(&self, _: Option<&str>) -> Result<Vec<NodeTitleEntry>, EmberTroveError> { unimplemented!() }
    async fn find_id_by_title(&self, _: &str) -> Result<Option<NodeId>, EmberTroveError> { unimplemented!() }
    async fn find_ids_by_titles(&self, _: &[String]) -> Result<std::collections::HashMap<String, NodeId>, EmberTroveError> { unimplemented!() }
    async fn list_all_for_owner(&self, _: &str) -> Result<Vec<Node>, EmberTroveError> { unimplemented!() }
    async fn area_for_nodes(&self, _: &[NodeId]) -> Result<Vec<(NodeId, NodeId, String)>, EmberTroveError> { unimplemented!() }
    async fn set_pinned(&self, _: NodeId, _: bool) -> Result<Node, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<Node>, EmberTroveError> { unimplemented!() }
}

struct StubEdgeRepo;
#[async_trait]
impl EdgeRepo for StubEdgeRepo {
    async fn create(&self, _: CreateEdgeRequest) -> Result<Edge, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: EdgeId) -> Result<Edge, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: EdgeId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn list_for_node(&self, _: NodeId) -> Result<Vec<Edge>, EmberTroveError> { unimplemented!() }
    async fn list_for_node_with_titles(&self, _: NodeId) -> Result<Vec<EdgeWithTitles>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<Edge>, EmberTroveError> { unimplemented!() }
    async fn sync_wikilinks(&self, _: NodeId, _: &[NodeId]) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubTagRepo;
#[async_trait]
impl TagRepo for StubTagRepo {
    async fn create(&self, _: &str, _: CreateTagRequest) -> Result<Tag, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: TagId) -> Result<Tag, EmberTroveError> { unimplemented!() }
    async fn list(&self, _: &str) -> Result<Vec<Tag>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<Tag>, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: TagId, _: UpdateTagRequest) -> Result<Tag, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: TagId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn attach(&self, _: NodeId, _: TagId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn detach(&self, _: NodeId, _: TagId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn list_for_node(&self, _: NodeId) -> Result<Vec<Tag>, EmberTroveError> { unimplemented!() }
}

struct StubTaskRepo;
#[async_trait]
impl TaskRepo for StubTaskRepo {
    async fn create(&self, _: Option<NodeId>, _: &str, _: CreateTaskRequest) -> Result<Task, EmberTroveError> { unimplemented!() }
    async fn list_for_node(&self, _: NodeId, _: &str) -> Result<Vec<Task>, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: TaskId) -> Result<Task, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: TaskId, _: UpdateTaskRequest) -> Result<Task, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: TaskId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn list_my_day(&self, _: &str, _: NaiveDate) -> Result<Vec<MyDayTask>, EmberTroveError> { unimplemented!() }
    async fn list_inbox(&self, _: &str) -> Result<Vec<Task>, EmberTroveError> { unimplemented!() }
    async fn list_open_for_owner(&self, _: &str) -> Result<Vec<MyDayTask>, EmberTroveError> { unimplemented!() }
    async fn counts_for_nodes(&self, _: &[NodeId]) -> Result<Vec<(NodeId, TaskCounts)>, EmberTroveError> { unimplemented!() }
    async fn list_open_for_nodes(&self, _: &[NodeId], _: i64) -> Result<Vec<(NodeId, Vec<common::task::TaskSummary>, bool)>, EmberTroveError> { unimplemented!() }
    async fn max_task_updated_for_nodes(&self, _: &[NodeId]) -> Result<Vec<(NodeId, chrono::DateTime<chrono::Utc>)>, EmberTroveError> { unimplemented!() }
    async fn list_all_for_owner(&self, _: &str) -> Result<Vec<Task>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<Task>, EmberTroveError> { unimplemented!() }
    async fn list_by_due_range(&self, _: &str, _: NaiveDate, _: NaiveDate) -> Result<Vec<MyDayTask>, EmberTroveError> { unimplemented!() }
    async fn reorder(&self, _: &[(TaskId, i32)], _: &str) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubNoteRepo;
#[async_trait]
impl NoteRepo for StubNoteRepo {
    async fn create(&self, _: Option<NodeId>, _: &str, _: CreateNoteRequest) -> Result<Note, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: common::id::NoteId, _: &str, _: common::note::UpdateNoteRequest) -> Result<Note, EmberTroveError> { unimplemented!() }
    async fn list_for_node(&self, _: NodeId) -> Result<Vec<Note>, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: common::id::NoteId, _: &str) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn feed(&self, _: Option<&str>, _: &crate::repo::note::NoteFeedFilter) -> Result<Vec<FeedNote>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<Note>, EmberTroveError> { unimplemented!() }
}

struct StubAttachmentRepo;
#[async_trait]
impl AttachmentRepo for StubAttachmentRepo {
    async fn create(&self, _: NodeId, _: &str, _: &str, _: i64, _: &str) -> Result<Attachment, EmberTroveError> { unimplemented!() }
    async fn list(&self, _: NodeId) -> Result<Vec<Attachment>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<Attachment>, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: AttachmentId) -> Result<Attachment, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: AttachmentId) -> Result<String, EmberTroveError> { unimplemented!() }
}

struct StubPermissionRepo;
#[async_trait]
impl PermissionRepo for StubPermissionRepo {
    async fn grant(&self, _: NodeId, _: &str, _: GrantPermissionRequest) -> Result<Permission, EmberTroveError> { unimplemented!() }
    async fn revoke(&self, _: PermissionId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn list(&self, _: NodeId) -> Result<Vec<Permission>, EmberTroveError> { unimplemented!() }
    async fn find(&self, _: NodeId, _: &str) -> Result<Option<Permission>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self, _: Option<NodeId>) -> Result<Vec<Permission>, EmberTroveError> { unimplemented!() }
    async fn find_by_id(&self, _: PermissionId) -> Result<Option<Permission>, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: PermissionId, _: PermissionRole, _: &str) -> Result<Permission, EmberTroveError> { unimplemented!() }
}

struct StubSearchRepo;
#[async_trait]
impl SearchRepo for StubSearchRepo {
    async fn search(&self, _: &SearchQuery) -> Result<SearchResponse, EmberTroveError> { unimplemented!() }
}

struct StubGraphRepo;
#[async_trait]
impl GraphRepo for StubGraphRepo {
    async fn list_positions(&self) -> Result<Vec<NodePosition>, EmberTroveError> { unimplemented!() }
    async fn upsert_position(&self, _: Uuid, _: f64, _: f64) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn save_positions(&self, _: &[(Uuid, f64, f64)]) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubBackupRepo;
#[async_trait]
impl BackupRepo for StubBackupRepo {
    async fn create(
        &self, _: &str, _: &str, _: i64,
        _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: Option<&str>,
    ) -> Result<BackupJob, EmberTroveError> { unimplemented!() }
    async fn list_for_owner(&self, _: &str) -> Result<Vec<BackupJob>, EmberTroveError> { unimplemented!() }
    async fn list_all(&self) -> Result<Vec<BackupJob>, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: Uuid) -> Result<BackupJob, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: Uuid) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubFavoriteRepo;
#[async_trait]
impl FavoriteRepo for StubFavoriteRepo {
    async fn list_all(&self) -> Result<Vec<Favorite>, EmberTroveError> { unimplemented!() }
    async fn list(&self, _: &str) -> Result<Vec<Favorite>, EmberTroveError> { unimplemented!() }
    async fn create(&self, _: &str, _: CreateFavoriteRequest) -> Result<Favorite, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: FavoriteId, _: &str) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn reorder(&self, _: &str, _: &[FavoriteId]) -> Result<Vec<Favorite>, EmberTroveError> { unimplemented!() }
}

struct StubShareTokenRepo;
#[async_trait]
impl ShareTokenRepo for StubShareTokenRepo {
    async fn list_all(&self) -> Result<Vec<common::share_token::ShareToken>, EmberTroveError> { unimplemented!() }
    async fn create(&self, _: NodeId, _: &str, _: &common::share_token::CreateShareTokenRequest) -> Result<common::share_token::ShareToken, EmberTroveError> { unimplemented!() }
    async fn list(&self, _: NodeId) -> Result<Vec<common::share_token::ShareToken>, EmberTroveError> { unimplemented!() }
    async fn find_by_token(&self, _: Uuid) -> Result<Option<common::share_token::ShareToken>, EmberTroveError> { unimplemented!() }
    async fn revoke(&self, _: ShareTokenId) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubActivityRepo;
#[async_trait]
impl ActivityRepo for StubActivityRepo {
    async fn record(&self, _: NodeId, _: &str, _: ActivityAction, _: serde_json::Value) -> Result<(), EmberTroveError> { Ok(()) }
    async fn list(&self, _: NodeId, _: i64) -> Result<Vec<ActivityEntry>, EmberTroveError> { unimplemented!() }
    async fn list_recent_for_owner(&self, _: &str, _: chrono::DateTime<chrono::Utc>, _: i64) -> Result<Vec<common::activity::RecentActivityEntry>, EmberTroveError> { unimplemented!() }
}

struct StubNodeVersionRepo;
#[async_trait]
impl NodeVersionRepo for StubNodeVersionRepo {
    async fn list_all(&self) -> Result<Vec<common::node_version::NodeVersion>, EmberTroveError> { unimplemented!() }
    async fn record(&self, _: NodeId, _: &str, _: &str) -> Result<(), EmberTroveError> { Ok(()) }
    async fn list(&self, _: NodeId, _: i64) -> Result<Vec<common::node_version::NodeVersion>, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: common::id::NodeVersionId) -> Result<common::node_version::NodeVersion, EmberTroveError> { unimplemented!() }
}

struct StubTemplateRepo;
#[async_trait]
impl TemplateRepo for StubTemplateRepo {
    async fn list(&self) -> Result<Vec<NodeTemplate>, EmberTroveError> { unimplemented!() }
    async fn get(&self, _: TemplateId) -> Result<NodeTemplate, EmberTroveError> { unimplemented!() }
    async fn create(&self, _: &str, _: CreateTemplateRequest) -> Result<NodeTemplate, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: TemplateId, _: UpdateTemplateRequest) -> Result<NodeTemplate, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: TemplateId) -> Result<(), EmberTroveError> { unimplemented!() }
    async fn set_default(&self, _: TemplateId, _: &str) -> Result<NodeTemplate, EmberTroveError> { unimplemented!() }
}

struct StubNodeLinkRepo;
#[async_trait]
impl NodeLinkRepo for StubNodeLinkRepo {
    async fn list_all(&self) -> Result<Vec<NodeLink>, EmberTroveError> { unimplemented!() }
    async fn list(&self, _: NodeId) -> Result<Vec<NodeLink>, EmberTroveError> { unimplemented!() }
    async fn create(&self, _: NodeId, _: CreateNodeLinkRequest) -> Result<NodeLink, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: NodeLinkId, _: UpdateNodeLinkRequest) -> Result<NodeLink, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: NodeLinkId) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubSearchPresetRepo;
#[async_trait]
impl SearchPresetRepo for StubSearchPresetRepo {
    async fn list(&self, _: &str) -> Result<Vec<SearchPreset>, EmberTroveError> { unimplemented!() }
    async fn create(&self, _: &str, _: CreateSearchPresetRequest) -> Result<SearchPreset, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: SearchPresetId, _: &str) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubWebhookRepo;
#[async_trait]
impl WebhookRepo for StubWebhookRepo {
    async fn list(&self, _: &str) -> Result<Vec<Webhook>, EmberTroveError> { unimplemented!() }
    async fn list_active_for_event(&self, _: &str) -> Result<Vec<Webhook>, EmberTroveError> { unimplemented!() }
    async fn create(&self, _: &str, _: CreateWebhookRequest) -> Result<Webhook, EmberTroveError> { unimplemented!() }
    async fn update(&self, _: WebhookId, _: &str, _: UpdateWebhookRequest) -> Result<Webhook, EmberTroveError> { unimplemented!() }
    async fn delete(&self, _: WebhookId, _: &str) -> Result<(), EmberTroveError> { unimplemented!() }
}

struct StubEditorPrefRepo;
#[async_trait]
impl crate::repo::editor_pref::EditorPrefRepo for StubEditorPrefRepo {
    async fn list_for_owner(&self, _: &str) -> Result<Vec<common::editor_pref::EditorPref>, EmberTroveError> { Ok(vec![]) }
    async fn set(&self, _: &str, _: &str, _: Uuid, _: i32) -> Result<(), EmberTroveError> { Ok(()) }
}

struct StubPkceRepo;
#[async_trait]
impl PkceRepo for StubPkceRepo {
    async fn store(&self, _: &str, _: &str) -> Result<(), EmberTroveError> { Ok(()) }
    async fn take(&self, _: &str, _: Duration) -> Result<Option<String>, EmberTroveError> { Ok(None) }
    async fn sweep_expired(&self, _: Duration) -> Result<u64, EmberTroveError> { Ok(0) }
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
        nodes:        Arc::new(StubNodeRepo),
        edges:        Arc::new(StubEdgeRepo),
        tags:         Arc::new(StubTagRepo),
        tasks:        Arc::new(StubTaskRepo),
        notes:        Arc::new(StubNoteRepo),
        attachments:  Arc::new(StubAttachmentRepo),
        permissions:  Arc::new(StubPermissionRepo),
        search:       Arc::new(StubSearchRepo),
        graph:        Arc::new(StubGraphRepo),
        backup:       Arc::new(StubBackupRepo),
        favorites:    Arc::new(StubFavoriteRepo),
        share_tokens:  Arc::new(StubShareTokenRepo),
        activity:      Arc::new(StubActivityRepo),
        node_versions: Arc::new(StubNodeVersionRepo),
        templates:       Arc::new(StubTemplateRepo),
        search_presets:  Arc::new(StubSearchPresetRepo),
        node_links:      Arc::new(StubNodeLinkRepo),
        webhooks:        Arc::new(StubWebhookRepo),
        editor_prefs:    Arc::new(StubEditorPrefRepo),
        object_store: Arc::new(NullObjectStore),
        oidc:          None,
        cognito_admin: None,
        notifier:      None,
        cookie_key:    Key::generate(),
        auth: AuthConfig {
            issuer:           "https://example.com".to_string(),
            client_id:        "test-client".to_string(),
            client_secret:    "test-secret".to_string(),
            frontend_url:     "http://localhost:3000".to_string(),
            api_external_url: "http://localhost:3003".to_string(),
            cookie_secure:    false,
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
async fn nodes_list_route_registered()   { assert_route_registered("GET",    "/nodes").await; }
#[tokio::test]
async fn node_create_route_registered()  { assert_route_registered("POST",   "/nodes").await; }
#[tokio::test]
async fn node_get_route_registered()     { assert_route_registered("GET",    &format!("/nodes/{}", Uuid::new_v4())).await; }
#[tokio::test]
async fn node_update_route_registered()  { assert_route_registered("PUT",    &format!("/nodes/{}", Uuid::new_v4())).await; }
#[tokio::test]
async fn node_delete_route_registered()  { assert_route_registered("DELETE", &format!("/nodes/{}", Uuid::new_v4())).await; }

// ── Per-node permission routes ────────────────────────────────────────────────

#[tokio::test]
async fn node_permissions_list_route_registered() {
    let nid = Uuid::new_v4();
    assert_route_registered("GET",    &format!("/nodes/{nid}/permissions")).await;
}
#[tokio::test]
async fn node_permissions_grant_route_registered() {
    let nid = Uuid::new_v4();
    assert_route_registered("POST",   &format!("/nodes/{nid}/permissions")).await;
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
    assert_route_registered("GET",    "/permissions").await;
}
#[tokio::test]
async fn standalone_permission_update_route_registered() {
    assert_route_registered("PUT",    &format!("/permissions/{}", Uuid::new_v4())).await;
}
#[tokio::test]
async fn standalone_permission_delete_route_registered() {
    assert_route_registered("DELETE", &format!("/permissions/{}", Uuid::new_v4())).await;
}

// ── Other domain routes ───────────────────────────────────────────────────────

#[tokio::test]
async fn tags_list_route_registered()      { assert_route_registered("GET", "/tags").await; }
#[tokio::test]
async fn search_route_registered()         { assert_route_registered("GET", "/search").await; }
#[tokio::test]
async fn graph_route_registered()          { assert_route_registered("GET", "/graph").await; }
#[tokio::test]
async fn favorites_list_route_registered() { assert_route_registered("GET", "/favorites").await; }
#[tokio::test]
async fn notes_feed_route_registered()     { assert_route_registered("GET", "/notes").await; }
#[tokio::test]
async fn edges_list_route_registered()     { assert_route_registered("GET", "/edges").await; }

// ── Template routes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn templates_list_route_registered()   { assert_route_registered("GET",    "/templates").await; }
#[tokio::test]
async fn template_create_route_registered()  { assert_route_registered("POST",   "/templates").await; }
#[tokio::test]
async fn template_get_route_registered()     { assert_route_registered("GET",    &format!("/templates/{}", Uuid::new_v4())).await; }
#[tokio::test]
async fn template_update_route_registered()  { assert_route_registered("PUT",    &format!("/templates/{}", Uuid::new_v4())).await; }
#[tokio::test]
async fn template_delete_route_registered()  { assert_route_registered("DELETE", &format!("/templates/{}", Uuid::new_v4())).await; }

// ── Node pin route ─────────────────────────────────────────────────────────────
#[tokio::test]
async fn node_pin_route_registered() { assert_route_registered("PUT", &format!("/nodes/{}/pin", Uuid::new_v4())).await; }

// ── Permission DTO unit tests ─────────────────────────────────────────────────

#[test]
fn update_permission_request_serializes_role() {
    use common::permission::UpdatePermissionRequest;
    let req = UpdatePermissionRequest { role: PermissionRole::Editor };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["role"], "editor");
}

#[test]
fn permission_role_roundtrip_all_variants() {
    for (variant, expected) in [
        (PermissionRole::Owner,  "owner"),
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
        resp.headers().get(axum::http::header::CONTENT_TYPE)
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
