-- S5a (#314, docs/ONTOLOGY.md §9.3): a Task is Open, Waiting, Done or
-- Cancelled, and Done carries its completion date.

-- `in_progress` becomes open (owner decision, 2026-10-30). That is a change
-- of vocabulary, not an edit: keep `updated_at`, or every project with such
-- a task would show on the dashboard as active today. The tasks trigger
-- learns the switch the nodes trigger has had since 035.
CREATE OR REPLACE FUNCTION update_node_tasks_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('ember.keep_updated_at', true) = 'on' THEN
        RETURN NEW;
    END IF;
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

SET LOCAL ember.keep_updated_at = 'on';
-- Deleted (tombstoned) tasks too: the CHECK below is for every row.
UPDATE node_tasks SET status = 'open' WHERE status = 'in_progress';

-- Postgres can't drop a value from an enum, so 'in_progress' stays in the
-- task_status type and a CHECK keeps it out of the table (as 040 did for
-- the Resource kind).
ALTER TABLE node_tasks
  ADD CONSTRAINT node_tasks_no_in_progress CHECK (status <> 'in_progress');

-- When the task was completed. Empty for every task completed before this
-- column existed: the date is never made up (owner decision, 2026-10-30).
ALTER TABLE node_tasks ADD COLUMN completed_at TIMESTAMPTZ;

-- Only a done task has a completion date (a cancelled task was not
-- completed, §9.3). The other direction, "a done task has one", is NOT a
-- CHECK on purpose: Postgres applies even a NOT VALID check to every later
-- update of an old row, so a task completed before this release could never
-- be renamed, reordered or deleted again. The API stamps the date in the
-- statement that sets the status (`repo/task.rs`).
ALTER TABLE node_tasks
  ADD CONSTRAINT node_tasks_completed_only_when_done
  CHECK (completed_at IS NULL OR status = 'done');
