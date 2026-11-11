use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{
    EmberTroveError,
    id::WebhookId,
    webhook::{CreateWebhookRequest, UpdateWebhookRequest, Webhook},
};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[async_trait]
pub trait WebhookRepo: Send + Sync + 'static {
    /// Return all webhooks owned by the given user.
    async fn list(&self, owner_id: &str) -> Result<Vec<Webhook>, EmberTroveError>;

    /// Return all active webhooks that subscribe to a given event.
    async fn list_active_for_event(&self, event: &str) -> Result<Vec<Webhook>, EmberTroveError>;

    /// Create a new webhook.
    async fn create(
        &self,
        owner_id: &str,
        req: CreateWebhookRequest,
    ) -> Result<Webhook, EmberTroveError>;

    /// Update a webhook — only owner may update.
    async fn update(
        &self,
        id: WebhookId,
        owner_id: &str,
        req: UpdateWebhookRequest,
    ) -> Result<Webhook, EmberTroveError>;

    /// Delete a webhook — only owner may delete.
    async fn delete(&self, id: WebhookId, owner_id: &str) -> Result<(), EmberTroveError>;
}

// ── Internal row type ────────────────────────────────────────────────────────

#[derive(FromRow)]
struct WebhookRow {
    id: Uuid,
    owner_id: String,
    url: String,
    secret: Option<String>,
    events: Vec<String>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl WebhookRow {
    fn into_webhook(self) -> Webhook {
        Webhook {
            id: WebhookId(self.id),
            owner_id: self.owner_id,
            url: self.url,
            secret: self.secret,
            events: self.events,
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// ── Postgres implementation ──────────────────────────────────────────────────

pub struct PgWebhookRepo {
    pool: PgPool,
}

impl PgWebhookRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Every owner's webhooks as a backup holds them (`backup::read_snapshot`).
/// SECURITY: the query never selects `secret`, only whether one is set, so
/// a signing secret can't reach an archive (owner decision, 2026-11-09).
/// Not on the trait: no route may read across owners.
pub(crate) async fn list_all_for_backup<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
) -> Result<Vec<common::backup::BackupWebhook>, EmberTroveError> {
    #[derive(FromRow)]
    struct Row {
        id: Uuid,
        owner_id: String,
        had_secret: bool,
        events: Vec<String>,
        is_active: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT id, owner_id, (secret IS NOT NULL) AS had_secret, events, is_active, \
                created_at, updated_at \
         FROM webhooks ORDER BY created_at, id",
    )
    .fetch_all(ex)
    .await
    .map_err(|e| EmberTroveError::Internal(format!("list all webhooks: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|r| common::backup::BackupWebhook {
            id: WebhookId(r.id),
            owner_id: r.owner_id,
            // Never selected: an address can be a credential.
            url: None,
            had_secret: r.had_secret,
            events: r.events,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

#[async_trait]
impl WebhookRepo for PgWebhookRepo {
    async fn list(&self, owner_id: &str) -> Result<Vec<Webhook>, EmberTroveError> {
        let rows = sqlx::query_as::<_, WebhookRow>(
            "SELECT id, owner_id, url, secret, events, is_active, created_at, updated_at \
             FROM webhooks WHERE owner_id = $1 ORDER BY created_at DESC",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list webhooks: {e}")))?;

        Ok(rows.into_iter().map(WebhookRow::into_webhook).collect())
    }

    async fn list_active_for_event(&self, event: &str) -> Result<Vec<Webhook>, EmberTroveError> {
        let rows = sqlx::query_as::<_, WebhookRow>(
            "SELECT id, owner_id, url, secret, events, is_active, created_at, updated_at \
             FROM webhooks WHERE is_active = TRUE AND $1 = ANY(events)",
        )
        .bind(event)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("list active webhooks: {e}")))?;

        Ok(rows.into_iter().map(WebhookRow::into_webhook).collect())
    }

    async fn create(
        &self,
        owner_id: &str,
        req: CreateWebhookRequest,
    ) -> Result<Webhook, EmberTroveError> {
        let row = sqlx::query_as::<_, WebhookRow>(
            "INSERT INTO webhooks (owner_id, url, secret, events) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, owner_id, url, secret, events, is_active, created_at, updated_at",
        )
        .bind(owner_id)
        .bind(&req.url)
        .bind(&req.secret)
        .bind(&req.events)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("create webhook: {e}")))?;

        Ok(row.into_webhook())
    }

    async fn update(
        &self,
        id: WebhookId,
        owner_id: &str,
        req: UpdateWebhookRequest,
    ) -> Result<Webhook, EmberTroveError> {
        // Secret PATCH semantics (see UpdateWebhookRequest): only touch the
        // stored secret when the field was present in the request. Clients
        // only ever receive the masked secret, so unconditionally writing
        // req.secret would wipe (or corrupt) it on every unrelated edit.
        let set_secret = req.secret.is_some();
        let secret_value: Option<String> = req.secret.clone().flatten();
        let row = sqlx::query_as::<_, WebhookRow>(
            "UPDATE webhooks \
             SET url = $1, \
                 secret = CASE WHEN $2 THEN $7 ELSE secret END, \
                 events = $3, is_active = $4, updated_at = now() \
             WHERE id = $5 AND owner_id = $6 \
             RETURNING id, owner_id, url, secret, events, is_active, created_at, updated_at",
        )
        .bind(&req.url)
        .bind(set_secret)
        .bind(&req.events)
        .bind(req.is_active)
        .bind(id.0)
        .bind(owner_id)
        .bind(secret_value)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmberTroveError::Internal(format!("update webhook: {e}")))?;

        row.map(WebhookRow::into_webhook)
            .ok_or_else(|| EmberTroveError::NotFound("webhook not found".to_string()))
    }

    async fn delete(&self, id: WebhookId, owner_id: &str) -> Result<(), EmberTroveError> {
        let result = sqlx::query("DELETE FROM webhooks WHERE id = $1 AND owner_id = $2")
            .bind(id.0)
            .bind(owner_id)
            .execute(&self.pool)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("delete webhook: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(EmberTroveError::NotFound("webhook not found".to_string()));
        }
        Ok(())
    }
}
