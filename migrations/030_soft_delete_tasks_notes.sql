-- Soft delete for tasks and notes (undo-toast support).
--
-- Deletes from the UI become `deleted_at = now()` tombstones so an undo toast
-- can restore them; every live-data query filters `deleted_at IS NULL`.
-- Tombstones older than 30 days are hard-deleted by the API (at startup and
-- once a day) — see TOMBSTONE_RETENTION_DAYS in api/src/main.rs.

ALTER TABLE node_tasks ADD COLUMN deleted_at TIMESTAMPTZ;
ALTER TABLE node_notes ADD COLUMN deleted_at TIMESTAMPTZ;

-- Partial indexes: tombstones are a tiny minority of rows; these keep the
-- purge scan and restore lookups cheap without taxing live-row queries.
CREATE INDEX idx_node_tasks_deleted_at ON node_tasks (deleted_at)
    WHERE deleted_at IS NOT NULL;
CREATE INDEX idx_node_notes_deleted_at ON node_notes (deleted_at)
    WHERE deleted_at IS NOT NULL;
