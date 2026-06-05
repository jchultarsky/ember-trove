use std::{sync::Arc, time::Instant};

use axum_extra::extract::cookie::Key;
use sqlx::PgPool;

use crate::{
    admin::CognitoAdminClient,
    auth::{AuthConfig, oidc::OidcClient},
    config::Config,
    notify::SesNotifier,
    object_store::ObjectStore,
    repo::{
        activity::ActivityRepo, attachment::AttachmentRepo, backup::BackupRepo, edge::EdgeRepo,
        editor_pref::EditorPrefRepo, favorite::FavoriteRepo, graph::GraphRepo, node::NodeRepo,
        node_link::NodeLinkRepo, node_version::NodeVersionRepo, note::NoteRepo,
        permission::PermissionRepo, pkce::PkceRepo, search::SearchRepo,
        search_presets::SearchPresetRepo, share_token::ShareTokenRepo, tag::TagRepo,
        task::TaskRepo, template::TemplateRepo, webhook::WebhookRepo,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub nodes: Arc<dyn NodeRepo>,
    pub edges: Arc<dyn EdgeRepo>,
    pub tags: Arc<dyn TagRepo>,
    pub tasks: Arc<dyn TaskRepo>,
    pub notes: Arc<dyn NoteRepo>,
    pub attachments: Arc<dyn AttachmentRepo>,
    pub permissions: Arc<dyn PermissionRepo>,
    pub search: Arc<dyn SearchRepo>,
    pub graph: Arc<dyn GraphRepo>,
    pub backup: Arc<dyn BackupRepo>,
    pub favorites: Arc<dyn FavoriteRepo>,
    pub share_tokens: Arc<dyn ShareTokenRepo>,
    pub activity: Arc<dyn ActivityRepo>,
    pub node_versions: Arc<dyn NodeVersionRepo>,
    pub templates: Arc<dyn TemplateRepo>,
    pub search_presets: Arc<dyn SearchPresetRepo>,
    pub node_links: Arc<dyn NodeLinkRepo>,
    pub webhooks: Arc<dyn WebhookRepo>,
    pub editor_prefs: Arc<dyn EditorPrefRepo>,
    pub object_store: Arc<dyn ObjectStore>,
    pub oidc: Option<Arc<OidcClient>>,
    /// Cognito admin client — `None` when `COGNITO_USER_POOL_ID` is not set.
    pub cognito_admin: Option<Arc<CognitoAdminClient>>,
    /// SES notifier — `None` when `SES_FROM_EMAIL` is not set.
    pub notifier: Option<Arc<SesNotifier>>,
    pub cookie_key: Key,
    pub auth: AuthConfig,
    pub config: Config,
    /// Process start time — used to compute uptime in the metrics endpoint.
    pub started_at: Instant,
    /// Process-lifetime HTTP request counters, surfaced in the metrics endpoint.
    pub request_metrics: Arc<crate::routes::metrics::RequestMetrics>,
    /// Short-lived PKCE verifier store — keyed by OAuth state parameter.
    /// Backed by Postgres so in-flight logins survive an API restart/redeploy.
    pub pkce: Arc<dyn PkceRepo>,
}

/// `PrivateCookieJar` needs `FromRef<AppState>` for `Key` to derive
/// the encryption key from shared state.
impl axum::extract::FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}
