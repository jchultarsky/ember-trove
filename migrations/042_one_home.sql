-- A node has at most one home (#228, ONTOLOGY §3.3, §6.1).
--
-- `contains` gives a node its home. Since v3.1.0 the API refuses a second
-- one under a lock; this adds the database rule behind it, so no path
-- (a restore, a future write, a race) can store two. Data from before
-- that may still hold them, so first the mechanical fallback the owner
-- chose (2026-10-31, 041 pattern):
--   * The OLDEST home of each node is kept (created_at, then id).
--   * Every other home is copied into `edges_removed_042`, then deleted.
--     Labels aren't carried: each belongs to its own parent.
--     `common::edge::fold_homes` is this rule in Rust, used by restore,
--     which archives the same way.
--   * `edges` is locked first, so no insert can slip a second home in
--     between the cleanup and the index build.
-- Idempotent: safe to re-run (the pg test re-runs it on seeded homes).

LOCK TABLE edges IN SHARE ROW EXCLUSIVE MODE;

CREATE TABLE IF NOT EXISTS edges_removed_042 (
    LIKE edges INCLUDING DEFAULTS,
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

DROP TABLE IF EXISTS homes_ranked_042;
CREATE TEMP TABLE homes_ranked_042 AS
SELECT id,
       row_number() OVER (PARTITION BY target_id ORDER BY created_at, id) AS rn
FROM edges
WHERE edge_type = 'contains';

INSERT INTO edges_removed_042 (id, source_id, target_id, edge_type, label, created_at)
SELECT e.id, e.source_id, e.target_id, e.edge_type, e.label, e.created_at
FROM edges e
JOIN homes_ranked_042 r ON r.id = e.id
WHERE r.rn > 1;

DELETE FROM edges e
USING homes_ranked_042 r
WHERE e.id = r.id AND r.rn > 1;

DROP TABLE homes_ranked_042;

CREATE UNIQUE INDEX IF NOT EXISTS edges_one_home
    ON edges (target_id)
    WHERE edge_type = 'contains';
