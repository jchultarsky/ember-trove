-- Captures get their own home (#234, ONTOLOGY §9.1, §9.2).
--
-- A capture is unclassified input waiting in the Inbox; a note belongs to
-- a node. Until now a capture was a note with no node (migration 028).
-- This moves those rows into `captures`, keeping each one's id, owner,
-- text, times and tombstone, and then makes a note's node required. A
-- capture has no colour; the notes moved here all used the default.
-- `common::capture::from_nodeless_note` is the same rule in Rust, for
-- restoring archives made before this (format 3).
-- Captures are owner-only: the API scopes every query to the owner and
-- gives the admin no bypass (owner decision, 2026-10-31).
-- Idempotent: safe to re-run (the pg test re-runs it on seeded notes).

CREATE TABLE IF NOT EXISTS captures (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id   TEXT        NOT NULL,
    body       TEXT        NOT NULL CHECK (char_length(body) BETWEEN 1 AND 10000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Soft delete, for Undo; tombstones are purged like tasks and notes.
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS captures_owner_created_idx
    ON captures (owner_id, created_at DESC)
    WHERE deleted_at IS NULL;

DROP TRIGGER IF EXISTS captures_updated_at ON captures;
CREATE TRIGGER captures_updated_at
    BEFORE UPDATE ON captures
    FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

LOCK TABLE node_notes IN SHARE ROW EXCLUSIVE MODE;

INSERT INTO captures (id, owner_id, body, created_at, updated_at, deleted_at)
SELECT id, owner_id, body, created_at, updated_at, deleted_at
FROM node_notes
WHERE node_id IS NULL
ON CONFLICT (id) DO NOTHING;

DELETE FROM node_notes WHERE node_id IS NULL;

ALTER TABLE node_notes ALTER COLUMN node_id SET NOT NULL;
