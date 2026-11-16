-- A task needs a Project (S5c, #335; ONTOLOGY §9.3: "exactly one Project.
-- No orphan Tasks", and §9.1: "the role of the orphan is played by a
-- capture").
--
-- Since migration 023 a task could have no node (the old Inbox). No page
-- has made one since Captures (3.5.0), and the API refuses one from this
-- release on. This moves the ones that are left into `captures` and then
-- makes a task's node required. It is the rule notes got in 043.
--
-- Each task with no node becomes a capture with the same id, owner,
-- times and tombstone: one in the trash is a trashed capture, purged on
-- the same schedule, so nothing is deleted early. Its text:
--   - still to do (open or waiting): the title, led by its due date if it
--     has one ("Fri 7 Nov 2025: call the bank");
--   - finished: "Done: " or "Cancelled: " and the title, so it doesn't
--     read as new work.
-- The title goes in as it is, not trimmed: a title of spaces must not
-- become an empty capture, which `captures` refuses.
-- `common::capture::nodeless_task_text` is the same rule in Rust, for
-- restoring an archive that still holds such a task; the pg test
-- `migration_054_…` holds the two to the same text.
--
-- Not carried, because a capture has no place for them: priority, the My
-- Day date, the order in a list, the completion date, and a finished
-- task's due date.
--
-- A task on a node is not touched, whatever the node's kind. "The node is
-- a Project" stays the API's rule (`Capability::Tasks`) and Data health's
-- (`task_on_non_project`); this migration only requires a node.
--
-- The way back: `ALTER TABLE node_tasks ALTER COLUMN node_id DROP NOT
-- NULL;` and `DELETE FROM _sqlx_migrations WHERE version = 54;` (the
-- previous release refuses to start while row 54 is listed). The captures
-- stay captures.
--
-- Idempotent: a second run finds no task without a node.

-- The ALTER below needs this lock anyway; taking it first means no lock
-- is upgraded while a writer waits.
LOCK TABLE node_tasks IN ACCESS EXCLUSIVE MODE;

INSERT INTO captures (id, owner_id, body, created_at, updated_at, deleted_at)
SELECT id,
       owner_id,
       CASE
           WHEN status = 'done' THEN 'Done: ' || title
           WHEN status = 'cancelled' THEN 'Cancelled: ' || title
           -- The cast keeps the session's time zone out of it.
           WHEN due_date IS NOT NULL
               THEN to_char(due_date::timestamp, 'Dy FMDD Mon YYYY') || ': ' || title
           ELSE title
       END,
       created_at,
       updated_at,
       deleted_at
FROM node_tasks
WHERE node_id IS NULL
ON CONFLICT (id) DO NOTHING;

DELETE FROM node_tasks WHERE node_id IS NULL;

ALTER TABLE node_tasks ALTER COLUMN node_id SET NOT NULL;
