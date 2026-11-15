-- S6d (docs/ONTOLOGY.md §9.2): "Notes are editable; their timestamp is not.
-- ... the note shows that it was edited."
--
-- Until now "edited" was worked out from `updated_at`, which the trigger
-- below moves on every write to the row: a real edit, but also a colour
-- change, a delete and the Undo that restores it. `edited_at` is set only
-- when the note's words change (`repo::note::update`).

-- The backfill below must not look like a change to each note. The notes
-- trigger learns the switch the nodes trigger has had since 035 and the
-- tasks trigger since 050.
CREATE OR REPLACE FUNCTION update_node_notes_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('ember.keep_updated_at', true) = 'on' THEN
        RETURN NEW;
    END IF;
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

ALTER TABLE node_notes ADD COLUMN edited_at TIMESTAMPTZ;

-- Existing marks are kept (owner decision, 2026-11-14): a note that shows
-- "edited" today keeps showing it, with the date the old mark carries. The
-- page showed it when the whole seconds between the two dates were more
-- than 2, which is three seconds or more. Some of these marks are wrong
-- (a restore or a colour change moved `updated_at` too); which ones can't
-- be recovered. The rule is `common::note::legacy_edited_at`; a test holds
-- this statement to it.
--
-- Live notes only: putting a note in the trash moved its `updated_at`, so
-- nearly every trashed note would be marked, and none of them showed a
-- mark (the trash shows no notes).
SET LOCAL ember.keep_updated_at = 'on';
UPDATE node_notes
   SET edited_at = updated_at
 WHERE deleted_at IS NULL
   AND updated_at - created_at >= interval '3 seconds';
