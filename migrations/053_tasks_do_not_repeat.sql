-- S5b (#332, docs/ONTOLOGY.md §9): a Task is a one-time action. The repeat
-- behaviour is removed, and a task that repeated becomes a one-time task.
--
-- Two columns go, and no row: `recurrence` (migration 022: the rule) and
-- `recurrence_parent_id` (migration 032: which completed task scheduled
-- this one). Dropping the second drops its index and its foreign key with
-- it. Every other column of every task, live or in the trash, stays as it
-- was, and no `updated_at` moves: a dropped column fires no trigger.
--
-- Nothing is kept aside (as `edges_removed_041` was): no task is removed,
-- and the rule is the one thing the ontology says a task no longer has.
-- To list what repeats before upgrading:
--   SELECT count(*) FROM node_tasks WHERE recurrence IS NOT NULL;
--
-- Deliberately not `IF EXISTS`: a database without these columns is not
-- the one this migration was written for.
--
-- The way back, by hand, to a release that still reads the columns (it
-- brings back the empty columns, not the rules):
--   ALTER TABLE node_tasks ADD COLUMN recurrence TEXT;
--   ALTER TABLE node_tasks
--       ADD COLUMN recurrence_parent_id UUID REFERENCES node_tasks (id) ON DELETE SET NULL;
--   CREATE INDEX idx_node_tasks_recurrence_parent ON node_tasks (recurrence_parent_id)
--       WHERE recurrence_parent_id IS NOT NULL;
--   DELETE FROM _sqlx_migrations WHERE version = 53;

ALTER TABLE node_tasks DROP COLUMN recurrence_parent_id;
ALTER TABLE node_tasks DROP COLUMN recurrence;
