-- The Project Brief (#282): where a project stands now.
--
-- One current Brief per project node; it goes with the node (cascade).
-- A replaced Brief is not kept here: its words move into History as a
-- `brief_superseded` activity entry (owner decision 2 on #282), so there
-- is one chronology and one purge path. `stated_at` is when this Brief
-- became current (the coalesce window starts there); `updated_at` is the
-- last save, the optimistic-concurrency version and the age shown.
-- The API enforces the limits in `common::brief`; the CHECKs here are a
-- partial backstop (no per-point length: a CHECK can't look inside an
-- array without a function). A NULL point is refused, because every read
-- decodes `points` as text and one NULL would fail them all; only a
-- hand-edited archive could carry one. The Project-only rule (§3.2) is enforced by the API, like
-- tasks and people. Additive only: safe to re-run.

CREATE TABLE IF NOT EXISTS project_briefs (
    node_id    UUID        PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
    headline   TEXT        CHECK (headline IS NULL OR char_length(headline) BETWEEN 1 AND 200),
    pitch      TEXT        NOT NULL DEFAULT '' CHECK (char_length(pitch) <= 4000),
    points     TEXT[]      NOT NULL DEFAULT '{}'
               CHECK (cardinality(points) <= 20 AND array_position(points, NULL) IS NULL),
    author_id  TEXT        NOT NULL,
    stated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The account purge rewrites authorship on other people's projects.
CREATE INDEX IF NOT EXISTS project_briefs_author_idx ON project_briefs (author_id);
