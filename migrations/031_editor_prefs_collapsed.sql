-- Per-user note collapse state (node page note panel: collapse a note to its
-- first line, persisted per account). Lives in editor_prefs with the other
-- per-item UI prefs so the core node_notes queries stay untouched.
--
-- `height` becomes optional: a note can be collapsed without ever having had
-- its editor resized. Existing rows keep their height and default to expanded.
ALTER TABLE editor_prefs ALTER COLUMN height DROP NOT NULL;
ALTER TABLE editor_prefs ADD COLUMN IF NOT EXISTS collapsed BOOLEAN NOT NULL DEFAULT FALSE;
