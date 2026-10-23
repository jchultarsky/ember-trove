-- Link each auto-scheduled occurrence of a recurring task to the instance
-- whose completion created it.
--
-- Completing a recurring task inserts its next occurrence. Reopening the
-- completed instance (an Undo, or simply unticking it) used to leave that
-- occurrence behind, so re-completing produced a duplicate. With the link the
-- API retracts (soft-deletes) the still-open occurrence on reopen.
--
-- Bookkeeping only: not exposed in the Task DTO and not carried by backups;
-- a restored or legacy row simply has no link (NULL), which disables the
-- retraction for it and nothing else.

ALTER TABLE node_tasks
    ADD COLUMN recurrence_parent_id UUID REFERENCES node_tasks (id) ON DELETE SET NULL;

CREATE INDEX idx_node_tasks_recurrence_parent ON node_tasks (recurrence_parent_id)
    WHERE recurrence_parent_id IS NOT NULL;
