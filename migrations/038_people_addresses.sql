-- #155: addresses become a list of labelled entries, like emails and phones
-- ([{"label": "home", "value": "1 Main St\nSpringfield", "preferred": true}]).
--
-- The single address (label + text) moves into a one-entry list, marked
-- preferred since it was the only one; a person without one gets []. The old
-- columns are dropped in the same transaction rather than left behind: a
-- stale copy of someone's address that edits no longer touch would be
-- personal data kept past its use. Visibility is unchanged (owner-only).
-- The CHECK mirrors `common::person::MAX_CONTACT_POINTS`.

ALTER TABLE people
    ADD COLUMN addresses JSONB NOT NULL DEFAULT '[]'
        CHECK (jsonb_typeof(addresses) = 'array' AND jsonb_array_length(addresses) <= 10);

-- A data move, not an edit: leave updated_at alone (see migration 035).
SET LOCAL ember.keep_updated_at = 'on';

UPDATE people
SET addresses = jsonb_build_array(jsonb_build_object(
        'label', coalesce(address_label, ''),
        'value', address,
        'preferred', true))
WHERE address IS NOT NULL AND btrim(address) <> '';

ALTER TABLE people
    DROP COLUMN address_label,
    DROP COLUMN address;
