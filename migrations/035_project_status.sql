-- #143: a project's status as one structured value instead of prose in a
-- `## Status` body section. Not `status`: that column is the Draft /
-- Published / Archived lifecycle.
--
-- Open values (the picker suggests ones already used), so TEXT rather than
-- an enum. The CHECK mirrors `common::project_status::normalize`: one
-- trimmed line of at most 40 characters.
ALTER TABLE nodes
    ADD COLUMN project_status TEXT NULL
        CONSTRAINT nodes_project_status_shape CHECK (
            project_status IS NULL
            OR (
                char_length(project_status) BETWEEN 1 AND 40
                AND project_status = btrim(project_status)
                AND project_status !~ '[[:cntrl:]]'
            )
        );

-- The node list's filter and the picker's suggestions both look up by
-- owner and case-insensitive value.
CREATE INDEX nodes_owner_project_status_idx
    ON nodes (owner_id, lower(project_status))
    WHERE project_status IS NOT NULL;

-- One-time data backfills that run in Rust at API startup, after the
-- migrations (`api/src/backfill.rs`). A row means that backfill ran; its
-- insert is the lock that keeps two starting instances from both running it.
CREATE TABLE data_backfills (
    name         TEXT        PRIMARY KEY,
    done_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    rows_changed INTEGER     NOT NULL DEFAULT 0
);

-- The 035 backfill moves each project's status token out of its body. That
-- is a system change, not an edit, so it must not bump `updated_at` (the
-- Recent lists would reshuffle). Inside a transaction that sets
-- `ember.keep_updated_at = 'on'` (SET LOCAL), the trigger leaves it alone.
CREATE OR REPLACE FUNCTION touch_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('ember.keep_updated_at', true) = 'on' THEN
        RETURN NEW;
    END IF;
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;
