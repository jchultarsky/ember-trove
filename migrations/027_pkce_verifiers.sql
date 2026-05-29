-- Server-side PKCE verifier store, keyed by the OAuth `state` parameter.
--
-- Replaces the previous in-memory `Arc<Mutex<HashMap>>` so that in-flight
-- logins survive an API restart / redeploy. The old store was wiped on every
-- process restart, producing `invalid_code_verifier` at /api/auth/callback for
-- any user mid-login during a deploy.
--
-- Rows are short-lived (10-minute TTL) and consumed exactly once at callback;
-- a background sweeper purges expired entries.
CREATE TABLE IF NOT EXISTS pkce_verifiers (
    state         TEXT        PRIMARY KEY,
    code_verifier TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Supports the periodic `DELETE ... WHERE created_at < cutoff` sweep.
CREATE INDEX IF NOT EXISTS idx_pkce_verifiers_created_at ON pkce_verifiers (created_at);
