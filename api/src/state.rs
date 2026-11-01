use std::{sync::Arc, time::Instant};

use axum_extra::extract::cookie::Key;
use sqlx::PgPool;

use crate::{
    admin::UserDirectory,
    auth::{AuthConfig, oidc::OidcClient, suspended::SuspendedSet},
    config::Config,
    notify::SesNotifier,
    object_store::ObjectStore,
    repo::{
        account::AccountRepo, activity::ActivityRepo, attachment::AttachmentRepo,
        backup::BackupRepo, data_health::DataHealthRepo, edge::EdgeRepo,
        editor_pref::EditorPrefRepo, favorite::FavoriteRepo, graph::GraphRepo, node::NodeRepo,
        node_link::NodeLinkRepo, node_version::NodeVersionRepo, note::NoteRepo,
        object_deletion::ObjectDeletionRepo, permission::PermissionRepo, person::PersonRepo,
        pkce::PkceRepo, search::SearchRepo, search_presets::SearchPresetRepo, tag::TagRepo,
        task::TaskRepo, template::TemplateRepo, webhook::WebhookRepo,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// The suspended-account list (#239).
    pub accounts: Arc<dyn AccountRepo>,
    /// The same list in memory, checked on every request (#239).
    pub suspended: SuspendedSet,
    pub nodes: Arc<dyn NodeRepo>,
    pub edges: Arc<dyn EdgeRepo>,
    pub tags: Arc<dyn TagRepo>,
    pub tasks: Arc<dyn TaskRepo>,
    pub notes: Arc<dyn NoteRepo>,
    /// Inbox captures (#234). Owner-scoped, no admin bypass.
    pub captures: Arc<dyn crate::repo::capture::CaptureRepo>,
    pub attachments: Arc<dyn AttachmentRepo>,
    pub permissions: Arc<dyn PermissionRepo>,
    /// People and memberships (#147). Owner-scoped, no admin bypass.
    pub people: Arc<dyn PersonRepo>,
    pub search: Arc<dyn SearchRepo>,
    pub graph: Arc<dyn GraphRepo>,
    pub backup: Arc<dyn BackupRepo>,
    pub favorites: Arc<dyn FavoriteRepo>,
    pub activity: Arc<dyn ActivityRepo>,
    /// The data-health report (#208). Owner-scoped, no admin bypass.
    pub data_health: Arc<dyn DataHealthRepo>,
    pub node_versions: Arc<dyn NodeVersionRepo>,
    pub templates: Arc<dyn TemplateRepo>,
    pub search_presets: Arc<dyn SearchPresetRepo>,
    pub node_links: Arc<dyn NodeLinkRepo>,
    pub webhooks: Arc<dyn WebhookRepo>,
    pub editor_prefs: Arc<dyn EditorPrefRepo>,
    pub object_store: Arc<dyn ObjectStore>,
    /// Stored files waiting to be deleted (#239).
    pub object_deletions: Arc<dyn ObjectDeletionRepo>,
    /// Set while a background drain runs, so deletes in a burst start one.
    pub draining: Arc<std::sync::atomic::AtomicBool>,
    pub oidc: Option<Arc<OidcClient>>,
    /// Cognito admin client — `None` when `COGNITO_USER_POOL_ID` is not set.
    pub cognito_admin: Option<Arc<dyn UserDirectory>>,
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

impl AppState {
    /// Delete queued files from storage in the background, now rather
    /// than at the next daily drain (#239). Best effort: what fails stays
    /// queued.
    pub fn drain_object_deletions(&self) {
        use std::sync::atomic::Ordering;
        // One at a time: a running drain keeps going until the queue is
        // empty, so a second would only repeat its deletes.
        if self
            .draining
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let queue = self.object_deletions.clone();
        let store = self.object_store.clone();
        let draining = self.draining.clone();
        tokio::spawn(async move {
            crate::object_store::outbox::drain(queue.as_ref(), store.as_ref()).await;
            draining.store(false, Ordering::Release);
        });
    }
}
