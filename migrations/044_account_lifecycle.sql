-- Account lifecycle (#239): suspending an account, and deleting its data.
--
-- `suspended_accounts` lists the accounts an admin suspended. The API keeps
-- it in memory and refuses their sessions at once, and Cognito blocks
-- their sign-in. It holds only the subject and who suspended it, no email
-- or other personal data. Deleting an account suspends it first and drops
-- the row last, so a deletion that stops halfway stays locked out.
--
-- `object_deletions` is an outbox of S3 objects to delete. A row is written
-- in the same transaction that deletes the row naming the object (an
-- account purge, a node delete), and a worker deletes the object and then
-- the row. A crash or an S3 failure leaves the row for the next try, so no
-- object is orphaned.
-- Idempotent: safe to re-run.

CREATE TABLE IF NOT EXISTS suspended_accounts (
    subject_id   TEXT        PRIMARY KEY,
    suspended_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    suspended_by TEXT        NOT NULL
);

CREATE TABLE IF NOT EXISTS object_deletions (
    s3_key          TEXT        PRIMARY KEY,
    enqueued_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempts        INTEGER     NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ
);
