-- S6a (#320, docs/ONTOLOGY.md §4, §4.1): a Project carries a small closed
-- state beside its free-text status label: active, parked, complete or
-- cancelled. Complete and cancelled are terminal; parked is not.

-- TEXT with a CHECK, not an enum type: Postgres can't drop an enum value
-- (040 and 050 each had to close one off with a CHECK), and this list is
-- one the ontology may still change.
ALTER TABLE nodes ADD COLUMN project_state TEXT NULL
  CONSTRAINT nodes_project_state_known
  CHECK (project_state IN ('active', 'parked', 'complete', 'cancelled'));

-- When a person last declared the state. Empty for every existing row,
-- and for a new project until someone declares one: the date is never
-- made up (the rule of node_tasks.completed_at, owner decision
-- 2026-10-30).
ALTER TABLE nodes ADD COLUMN project_state_at TIMESTAMPTZ NULL;

-- Existing labels are read once (owner decision, 2026-10-30): a label that
-- IS one of these words gives the state; everything else is active. From
-- here on the state is declared and never read from the label (§4.1).
-- No label gives "parked": parking is declared by a person, and a
-- migration that parked projects would take them out of their person's
-- Area groups and Today on the day of the deploy. Labels that only
-- suggest a state are listed in Data health instead.
--
-- `common::project_state::state_from_label` is this statement's twin; a
-- Postgres test holds the two together. The C collation folds ASCII case
-- only, as the Rust side does.
--
-- A change of vocabulary, not an edit: keep `updated_at` (it is the
-- version an open editor saves against, and the dashboard's "last
-- activity").
SET LOCAL ember.keep_updated_at = 'on';
UPDATE nodes
SET project_state = CASE
      WHEN lower(btrim(project_status, ' ') COLLATE "C")
           IN ('done', 'complete', 'completed') THEN 'complete'
      WHEN lower(btrim(project_status, ' ') COLLATE "C")
           IN ('cancelled', 'canceled', 'abandoned') THEN 'cancelled'
      ELSE 'active'
    END
WHERE node_type = 'project';

-- A Project always has a state and nothing else ever does (a node's kind
-- never changes, §3.3). So a write that forgets the column fails here,
-- loudly, instead of quietly re-opening a closed project.
ALTER TABLE nodes
  ADD CONSTRAINT nodes_project_state_only_on_projects
  CHECK ((node_type = 'project') = (project_state IS NOT NULL));

ALTER TABLE nodes
  ADD CONSTRAINT nodes_project_state_at_needs_a_state
  CHECK (project_state_at IS NULL OR project_state IS NOT NULL);
