-- Remove wiki-link edges that cross owners (security hotfix, v2.31.1).
--
-- Wiki-link titles used to resolve against EVERY owner's nodes, so writing
-- `[[Someone Else's Title]]` created an edge into another user's private
-- node, and `GET /nodes/{id}/neighbors` then returned that node's body.
-- Resolution is now owner-scoped and the neighbor/backlink/edge reads filter
-- by the caller's visibility, so these rows are both unreachable and invalid.
--
-- Only machine-generated `wiki_link` edges are removed: they are derived from
-- node bodies and are re-created (correctly scoped) on the next save. Edges a
-- user created explicitly are left alone; the read-side visibility filter
-- already keeps them from exposing anything.

DELETE FROM edges e
USING nodes s, nodes t
WHERE e.edge_type = 'wiki_link'
  AND s.id = e.source_id
  AND t.id = e.target_id
  AND s.owner_id <> t.owner_id;
