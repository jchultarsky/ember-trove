-- When an account's admin role was taken away (2026-11-10).
--
-- An ID token is checked locally, and its groups are read from the token.
-- So one issued while the account was an admin went on carrying the admin
-- role for up to its lifetime after an admin removed it, long enough for
-- the demoted account to give itself the role back. The API keeps these
-- rows in memory and drops the admin role from any token issued at or
-- before `demoted_at` (`auth::demoted`). A row is only needed while such a
-- token can still be alive; the daily sweep drops it after a day, the
-- longest a Cognito ID token lives. Holds the subject and a time, no email
-- or other personal data. Additive. Idempotent: safe to re-run.

CREATE TABLE IF NOT EXISTS admin_demotions (
    subject_id TEXT        PRIMARY KEY,
    demoted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
