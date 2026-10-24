-- Remove wiki-link edges into nodes their source's owner cannot see
-- (security hotfix, v2.31.1).
--
-- Wiki-link titles used to resolve against EVERY owner's nodes, so writing
-- `[[Someone Else's Title]]` created an edge into another user's private
-- node, and `GET /nodes/{id}/neighbors` then returned that node's body.
-- Resolution is now limited to nodes visible to the source's owner (their own
-- plus nodes shared with them), and the neighbor/backlink/edge reads filter by
-- the caller's visibility.
--
-- Only edges that could never have been legitimate are removed: machine-made
-- `wiki_link` edges whose target is neither owned by nor shared with the
-- source node's owner. Links into deliberately shared nodes are KEPT, and so
-- is every edge a user created explicitly.
--
-- Reversible: every removed row is first copied into `edges_removed_033`
-- (same columns + removed_at). To undo:
--   INSERT INTO edges (id, source_id, target_id, edge_type, label, created_at)
--   SELECT id, source_id, target_id, edge_type, label, created_at
--   FROM edges_removed_033 ON CONFLICT (id) DO NOTHING;
-- Drop the archive table in a later migration once it is no longer needed.

CREATE TABLE IF NOT EXISTS edges_removed_033 (
    LIKE edges INCLUDING DEFAULTS,
    removed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO edges_removed_033 (id, source_id, target_id, edge_type, label, created_at)
SELECT e.id, e.source_id, e.target_id, e.edge_type, e.label, e.created_at
FROM edges e
JOIN nodes s ON s.id = e.source_id
JOIN nodes t ON t.id = e.target_id
WHERE e.edge_type = 'wiki_link'
  AND t.owner_id <> s.owner_id
  AND NOT EXISTS (
      SELECT 1 FROM permissions p
      WHERE p.node_id = t.id AND p.subject_id = s.owner_id
  );

DELETE FROM edges e
USING edges_removed_033 r
WHERE e.id = r.id;
