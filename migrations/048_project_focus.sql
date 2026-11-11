-- Focus (#282, proposal B): the projects that hold a person's attention.
--
-- One row per account and project. Focus is private to the account
-- (`subject_id`): the API matches the caller on every query and gives the
-- admin no bypass, like captures and people (owner decision, 2026-11-09).
-- It is not a column on `nodes`: that would show one person's Focus to
-- everyone who can see the node, and allow only one person to have it.
--
-- `position` is the person's own order; a new entry goes last. `why_now` is
-- their optional one-line reason, never shown to anyone else. `added_at` is
-- when the project entered Focus (the card shows "In focus 6d").
--
-- A row goes with its node (cascade), with its account (the purge), or when
-- its person removes it. Nothing else deletes one: a project that is
-- archived, or that the person can no longer see, is left out by the read
-- (`common::project_focus::holds_focus`). The Project-only rule (ONTOLOGY
-- §3.2) is enforced by the API, like tasks and the Brief.
--
-- The API enforces the why-line's limits in `common::project_focus`; the
-- CHECK is a backstop. Additive. Idempotent: safe to re-run.

CREATE TABLE IF NOT EXISTS project_focus (
    subject_id TEXT        NOT NULL,
    node_id    UUID        NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    position   INTEGER     NOT NULL DEFAULT 0,
    why_now    TEXT        CHECK (why_now IS NULL OR
                   (char_length(why_now) BETWEEN 1 AND 200
                    AND position(E'\n' IN why_now) = 0
                    AND position(E'\r' IN why_now) = 0)),
    added_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (subject_id, node_id)
);

-- The node's cascade finds its rows by node.
CREATE INDEX IF NOT EXISTS project_focus_node_idx ON project_focus (node_id);
