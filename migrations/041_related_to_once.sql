-- `related_to` is stored once and unordered (#226, ONTOLOGY §6.2).
--
-- It is the one symmetric edge type: its whole meaning is that the two
-- nodes can be reached from each other. A→B and B→A are the same edge, but
-- `edges` stored them as two (`edges_unique_source_target_type` is
-- directed). Same rule as migration 034, per unordered pair:
--   * The OLDEST edge of each pair is kept. If it has no label, it takes the
--     oldest non-empty label of the rest. When both have labels, the kept
--     one keeps its own and the other's survives only in the archive.
--     `common::edge::fold_related_pairs` is this rule in Rust, used by
--     restore, which archives the same way.
--   * Every removed row is first copied into `edges_removed_041`.
--   * `edges` is locked first, so no insert can slip a reverse pair in
--     between the dedupe and the index build.
-- Idempotent: safe to re-run (the pg test re-runs it on seeded pairs).

LOCK TABLE edges IN SHARE ROW EXCLUSIVE MODE;

CREATE TABLE IF NOT EXISTS edges_removed_041 (
    LIKE edges INCLUDING DEFAULTS,
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

DROP TABLE IF EXISTS related_ranked_041;
CREATE TEMP TABLE related_ranked_041 AS
SELECT id, source_id, target_id, label, created_at,
       LEAST(source_id, target_id)    AS lo,
       GREATEST(source_id, target_id) AS hi,
       row_number() OVER (
           PARTITION BY LEAST(source_id, target_id), GREATEST(source_id, target_id)
           ORDER BY created_at, id
       ) AS rn
FROM edges
WHERE edge_type = 'related_to';

-- Carry the oldest non-empty label onto a kept edge that has none.
UPDATE edges e
SET label = carried.label
FROM (
    SELECT DISTINCT ON (k.id) k.id, d.label
    FROM related_ranked_041 k
    JOIN related_ranked_041 d ON d.lo = k.lo AND d.hi = k.hi AND d.rn > 1
    WHERE k.rn = 1
      AND (k.label IS NULL OR btrim(k.label) = '')
      AND d.label IS NOT NULL AND btrim(d.label) <> ''
    ORDER BY k.id, d.created_at, d.id
) carried
WHERE e.id = carried.id;

INSERT INTO edges_removed_041 (id, source_id, target_id, edge_type, label, created_at)
SELECT id, source_id, target_id, 'related_to', label, created_at
FROM related_ranked_041
WHERE rn > 1;

DELETE FROM edges e
USING related_ranked_041 r
WHERE e.id = r.id AND r.rn > 1;

DROP TABLE related_ranked_041;

CREATE UNIQUE INDEX IF NOT EXISTS edges_related_to_once
    ON edges (LEAST(source_id, target_id), GREATEST(source_id, target_id))
    WHERE edge_type = 'related_to';
