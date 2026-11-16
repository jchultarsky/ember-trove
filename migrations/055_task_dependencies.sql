-- S5d (ONTOLOGY §9.3, §6.4): a task can wait for another task.
--
-- One row says "task_id cannot proceed until depends_on_id is Done". It
-- is a hard prerequisite that the app surfaces and never enforces: a
-- blocked task says so, and nothing is prevented or reordered.
--
-- The row belongs to its two tasks, and to no account: there is no owner
-- column, so the account purge has nothing to clean here. It goes when
-- either task is deleted for good (the cascade): by the trash being
-- emptied, by its node being deleted, or by its account being purged. A
-- task in the trash keeps its rows; the reads leave them out, and
-- restoring the task brings them back.
--
-- What the database holds:
--   - a pair once (the primary key);
--   - never a task waiting for itself (the CHECK).
-- What it can't hold in a constraint, and the API checks under an
-- advisory lock (`repo::task_dependency::add`): no loop. A loop of
-- dependencies is a deadlock, not a storable state.
--
-- Who may read or change a row is decided through the tasks' nodes
-- (§14), in the API. Additive. Idempotent: safe to re-run.

CREATE TABLE IF NOT EXISTS task_dependencies (
    task_id       UUID        NOT NULL REFERENCES node_tasks(id) ON DELETE CASCADE,
    depends_on_id UUID        NOT NULL REFERENCES node_tasks(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, depends_on_id),
    CONSTRAINT task_dependencies_not_itself CHECK (task_id <> depends_on_id)
);

-- "What waits for this task": the Blocks list, the loop check's walk
-- upwards, and the cascade from the prerequisite's side. The primary key
-- serves the other direction.
CREATE INDEX IF NOT EXISTS task_dependencies_depends_on_idx
    ON task_dependencies (depends_on_id);
