mod admin;
mod auth;
mod backfill;
mod backup;
mod config;
mod error;
mod notify;
mod object_store;
mod repo;
mod routes;
mod ssrf;
mod state;
mod webhook_dispatch;

#[cfg(test)]
mod tests;

use std::{net::SocketAddr, sync::Arc};

use axum_extra::extract::cookie::Key;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use admin::CognitoAdminClient;
use auth::{AuthConfig, oidc::OidcClient};
use config::Config;
use notify::SesNotifier;
use object_store::s3::S3ObjectStore;
use repo::{
    activity::PgActivityRepo, attachment::PgAttachmentRepo, backup::PgBackupRepo, edge::PgEdgeRepo,
    editor_pref::PgEditorPrefRepo, favorite::PgFavoriteRepo, graph::PgGraphRepo, node::PgNodeRepo,
    node_link::PgNodeLinkRepo, node_version::PgNodeVersionRepo, note::PgNoteRepo,
    permission::PgPermissionRepo, pkce::PgPkceRepo, search::PgSearchRepo,
    search_presets::PgSearchPresetRepo, tag::PgTagRepo, task::PgTaskRepo, template::PgTemplateRepo,
    webhook::PgWebhookRepo,
};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()?;

    let pool = repo::pool_options()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    tracing::info!("database connection pool established");

    // Run pending migrations on startup.
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("database migration failed: {e}"))?;
    tracing::info!("database migrations complete");
    backfill::run_all(&pool).await;

    // OIDC discovery — fetch endpoints and JWKS from Cognito (optional for local dev).
    let oidc = if let (Some(issuer), Some(client_id), Some(client_secret)) = (
        &config.oidc_issuer,
        &config.oidc_client_id,
        &config.oidc_client_secret,
    ) {
        let client = OidcClient::discover(issuer, client_id.clone(), client_secret.clone()).await?;

        Some(Arc::new(client))
    } else {
        tracing::warn!("OIDC not configured — auth endpoints will be disabled");
        None
    };

    let auth = AuthConfig {
        issuer: config.oidc_issuer.clone().unwrap_or_default(),
        client_id: config.oidc_client_id.clone().unwrap_or_default(),
        client_secret: config.oidc_client_secret.clone().unwrap_or_default(),
        frontend_url: config.frontend_url.clone(),
        api_external_url: config.api_external_url.clone(),
        cookie_secure: config.cookie_secure,
    };

    // Derive cookie encryption key from hex-encoded COOKIE_KEY.
    let key_bytes = hex::decode(&config.cookie_key)
        .map_err(|e| anyhow::anyhow!("COOKIE_KEY is not valid hex: {e}"))?;

    // SECURITY: Reject trivially weak keys (all zeros, all same byte).
    if key_bytes.iter().all(|&b| b == key_bytes[0]) {
        anyhow::bail!(
            "COOKIE_KEY is trivially weak (all identical bytes). \
             Generate a secure key with: openssl rand -hex 64"
        );
    }

    let cookie_key = Key::from(&key_bytes);

    let object_store: Arc<dyn object_store::ObjectStore> =
        if let (Some(bucket), Some(access_key), Some(secret_key)) = (
            config.s3_bucket.as_deref(),
            config.s3_access_key.as_deref(),
            config.s3_secret_key.as_deref(),
        ) {
            Arc::new(
                S3ObjectStore::new(
                    bucket,
                    &config.s3_region,
                    access_key,
                    secret_key,
                    config.s3_endpoint.as_deref(),
                )
                .map_err(|e| anyhow::anyhow!("S3 init failed: {e}"))?,
            )
        } else {
            tracing::warn!("S3 not configured — attachment upload/download will be unavailable");
            Arc::new(object_store::NullObjectStore)
        };

    // Cognito Admin client — optional; enabled when COGNITO_USER_POOL_ID is set.
    let cognito_admin = if let Some(pool_id) = config.cognito_user_pool_id.clone() {
        tracing::info!("Cognito admin client enabled (pool: {pool_id})");
        let client: Arc<dyn admin::UserDirectory> =
            Arc::new(CognitoAdminClient::new(&config.cognito_region, pool_id).await);
        Some(client)
    } else {
        tracing::info!("COGNITO_USER_POOL_ID not set — /admin/* endpoints disabled");
        None
    };

    // SES notifier — optional; enabled when SES_FROM_EMAIL is set.
    let notifier = if let Some(from_email) = config.ses_from_email.clone() {
        tracing::info!("SES notifier enabled (from: {from_email})");
        Some(Arc::new(
            SesNotifier::new(from_email, config.frontend_url.clone()).await,
        ))
    } else {
        tracing::info!("SES_FROM_EMAIL not set — invite email notifications disabled");
        None
    };

    // Suspended accounts (#239). A failed load stops startup, like a failed
    // migration: serving without the list would let suspended sessions in.
    let accounts: Arc<dyn repo::account::AccountRepo> =
        Arc::new(repo::account::PgAccountRepo::new(pool.clone()));
    let suspended = auth::suspended::SuspendedSet::new(
        accounts
            .suspended()
            .await
            .map_err(|e| anyhow::anyhow!("loading suspended accounts failed: {e}"))?,
    );

    let state = AppState {
        accounts,
        suspended,
        started_at: std::time::Instant::now(),
        request_metrics: Arc::new(routes::metrics::RequestMetrics::default()),
        nodes: Arc::new(PgNodeRepo::new(pool.clone())),
        edges: Arc::new(PgEdgeRepo::new(pool.clone())),
        tags: Arc::new(PgTagRepo::new(pool.clone())),
        tasks: Arc::new(PgTaskRepo::new(pool.clone())),
        notes: Arc::new(PgNoteRepo::new(pool.clone())),
        captures: Arc::new(repo::capture::PgCaptureRepo::new(pool.clone())),
        attachments: Arc::new(PgAttachmentRepo::new(pool.clone())),
        permissions: Arc::new(PgPermissionRepo::new(pool.clone())),
        people: Arc::new(repo::person::PgPersonRepo::new(pool.clone())),
        data_health: Arc::new(repo::data_health::PgDataHealthRepo::new(pool.clone())),
        search: Arc::new(PgSearchRepo::new(pool.clone())),
        graph: Arc::new(PgGraphRepo::new(pool.clone())),
        backup: Arc::new(PgBackupRepo::new(pool.clone())),
        favorites: Arc::new(PgFavoriteRepo::new(pool.clone())),
        activity: Arc::new(PgActivityRepo::new(pool.clone())),
        node_versions: Arc::new(PgNodeVersionRepo::new(pool.clone())),
        templates: Arc::new(PgTemplateRepo::new(pool.clone())),
        search_presets: Arc::new(PgSearchPresetRepo::new(pool.clone())),
        node_links: Arc::new(PgNodeLinkRepo::new(pool.clone())),
        webhooks: Arc::new(PgWebhookRepo::new(pool.clone())),
        editor_prefs: Arc::new(PgEditorPrefRepo::new(pool.clone())),
        object_store,
        object_deletions: Arc::new(repo::object_deletion::PgObjectDeletionRepo::new(
            pool.clone(),
        )),
        draining: Arc::default(),
        oidc,
        cognito_admin,
        notifier,
        cookie_key,
        auth,
        config: config.clone(),
        pkce: Arc::new(PgPkceRepo::new(pool.clone())),
        pool,
    };

    // Spawn a background task that sweeps expired PKCE entries every 5 minutes.
    // Without this, abandoned OAuth flows accumulate unboundedly in the table.
    {
        let pkce = state.pkce.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                match pkce.sweep_expired(repo::pkce::PKCE_TTL).await {
                    Ok(removed) if removed > 0 => {
                        tracing::debug!(removed, "PKCE store: swept expired entries");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(?e, "PKCE sweep failed"),
                }
            }
        });
    }

    // Re-read the suspended list, so a change made by another instance (or
    // directly in the table) applies within a minute (#239).
    {
        let accounts = state.accounts.clone();
        let suspended = state.suspended.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(auth::suspended::REFRESH_INTERVAL);
            interval.tick().await; // the first tick is immediate; startup just loaded it
            loop {
                interval.tick().await;
                if let Err(e) = suspended.refresh(|| accounts.suspended()).await {
                    tracing::warn!(
                        ?e,
                        "suspended accounts refresh failed; keeping the last list"
                    );
                }
            }
        });
    }

    // Purge task/note tombstones older than the retention window, at startup
    // (first interval tick fires immediately) and then daily. Soft-deleted
    // rows back the UI's undo toast; after 30 days "deleted means deleted".
    {
        /// How long soft-deleted tasks/notes are kept before hard deletion.
        const TOMBSTONE_RETENTION_DAYS: i64 = 30;
        let tasks = state.tasks.clone();
        let notes = state.notes.clone();
        let captures = state.captures.clone();
        let accounts = state.accounts.clone();
        let object_deletions = state.object_deletions.clone();
        let object_store = state.object_store.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
            loop {
                interval.tick().await;
                let cutoff = chrono::Utc::now() - chrono::TimeDelta::days(TOMBSTONE_RETENTION_DAYS);
                match tasks.purge_deleted_before(cutoff).await {
                    Ok(purged) if purged > 0 => {
                        tracing::info!(purged, "purged expired task tombstones");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(?e, "task tombstone purge failed"),
                }
                match notes.purge_deleted_before(cutoff).await {
                    Ok(purged) if purged > 0 => {
                        tracing::info!(purged, "purged expired note tombstones");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(?e, "note tombstone purge failed"),
                }
                match captures.purge_deleted_before(cutoff).await {
                    Ok(purged) if purged > 0 => {
                        tracing::info!(purged, "purged expired capture tombstones");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(?e, "capture tombstone purge failed"),
                }
                // A deleted account stays refused for a day (#239): the
                // longest a Cognito ID token issued before the deletion lives.
                let token_cutoff = chrono::Utc::now() - chrono::TimeDelta::days(1);
                // First purge each deleted account again: anything written
                // after its purge (a request already in flight) goes too.
                match accounts.deleted().await {
                    Ok(subjects) => {
                        for subject in subjects {
                            if let Err(e) = accounts.purge(&subject, None).await {
                                tracing::warn!(?e, "re-purging a deleted account failed");
                            }
                        }
                    }
                    Err(e) => tracing::warn!(?e, "listing deleted accounts failed"),
                }
                match accounts.purge_deleted_before(token_cutoff).await {
                    Ok(purged) if purged > 0 => {
                        tracing::info!(purged, "dropped expired deleted-account rows");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(?e, "deleted-account row purge failed"),
                }
                // Retry stored files that couldn't be deleted yet (#239).
                object_store::outbox::drain(object_deletions.as_ref(), object_store.as_ref()).await;
            }
        });
    }

    let app = axum::Router::new().nest("/api", routes::build_router(state)?);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("ember-trove-api listening on {addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
