-- Allow standalone (inbox / micro-blog) notes that aren't tied to a node.
--
-- Mirrors migration 023 which made node_tasks.node_id nullable for inbox tasks.
-- Node-attached notes keep ON DELETE CASCADE; standalone notes have node_id NULL
-- and survive independently of any node.
ALTER TABLE node_notes ALTER COLUMN node_id DROP NOT NULL;

-- The central feed lists a user's notes newest-first across all nodes; this
-- index supports that owner-scoped, created_at-ordered scan (and the upcoming
-- node/date filters in the Notes view).
CREATE INDEX IF NOT EXISTS idx_node_notes_owner_created
    ON node_notes (owner_id, created_at DESC);
