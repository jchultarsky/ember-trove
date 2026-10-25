-- One edge per (source, target, type) (#129).
--
-- `POST /edges` inserted unconditionally and `edges` had no uniqueness
-- constraint, so submitting the Connections form (or the graph's New Edge
-- dialog) twice for the same pair and type created identical edges. An edge
-- IS the relationship (source, target, type); the label annotates it, so two
-- same-type edges with different labels are still duplicates. A different
-- TYPE between the same nodes is a different relationship and stays allowed.
--
-- Protecting user data:
--   * The OLDEST edge of each duplicate group is kept. If it has no label,
--     it takes the oldest non-empty label among its duplicates, so no
--     annotation is lost from the live graph.
--   * Every removed row is first copied into `edges_removed_034` (same
--     columns + removed_at). Restoring one needs the unique index gone or a
--     different (source, target, type); e.g. to inspect:
--       SELECT * FROM edges_removed_034 ORDER BY source_id, removed_at;
-- Idempotent: safe to re-run (tests re-run it on seeded duplicates).

CREATE TABLE IF NOT EXISTS edges_removed_034 (
    LIKE edges INCLUDING DEFAULTS,
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Rank each edge within its (source, target, type) group, oldest first.
DROP TABLE IF EXISTS edges_ranked_034;
CREATE TEMP TABLE edges_ranked_034 AS
SELECT id, source_id, target_id, edge_type, label, created_at,
       row_number() OVER (
           PARTITION BY source_id, target_id, edge_type
           ORDER BY created_at, id
       ) AS rn
FROM edges;

-- Carry the oldest non-empty label onto a kept edge that has none.
UPDATE edges e
SET label = carried.label
FROM (
    SELECT DISTINCT ON (k.id) k.id, d.label
    FROM edges_ranked_034 k
    JOIN edges_ranked_034 d
      ON d.source_id = k.source_id AND d.target_id = k.target_id
     AND d.edge_type = k.edge_type AND d.rn > 1
    WHERE k.rn = 1
      AND (k.label IS NULL OR btrim(k.label) = '')
      AND d.label IS NOT NULL AND btrim(d.label) <> ''
    ORDER BY k.id, d.created_at, d.id
) carried
WHERE e.id = carried.id;

INSERT INTO edges_removed_034 (id, source_id, target_id, edge_type, label, created_at)
SELECT id, source_id, target_id, edge_type, label, created_at
FROM edges_ranked_034
WHERE rn > 1
ON CONFLICT DO NOTHING;

DELETE FROM edges e
USING edges_ranked_034 r
WHERE e.id = r.id AND r.rn > 1;

DROP TABLE edges_ranked_034;

CREATE UNIQUE INDEX IF NOT EXISTS edges_unique_source_target_type
    ON edges (source_id, target_id, edge_type);
