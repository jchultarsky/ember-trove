-- S8a (#353; ONTOLOGY §7.1): email is identity. The same address means the
-- same person, so a save may not give two people of one account one
-- address. Until now the app warned and offered "Save anyway".
--
-- The rule lives here, as a trigger, so it holds for every writer: the
-- API, a restore, a future merge tool. It is not a unique index, because
-- of the owner's decision of 2026-11-18: people who ALREADY share an
-- address stay as they are and stay editable (they are listed in Data
-- health until someone edits or merges them). An index can't say "old
-- pairs may stay, new ones may not". One can replace this trigger in a
-- later migration, once no address is shared.
--
-- This migration writes no data and changes no row.

-- What makes two addresses the same: the spaces around it off, ASCII
-- letters without case. `COLLATE "C"` on purpose: how other letters are
-- lower-cased turns on the database's collation, and the answer must be
-- the same on every installation. Its twin is `common::person::email_key`.
CREATE OR REPLACE FUNCTION person_email_key(value TEXT) RETURNS TEXT
LANGUAGE sql IMMUTABLE PARALLEL SAFE
AS $$ SELECT lower(btrim(value) COLLATE "C") $$;

-- The other people of account `p_owner` who list an address that
-- `p_new` holds and `p_old` did not: what a save of `p_self` would
-- collide with. `p_self` is NULL for a person not yet written.
--
-- SECURITY: `p.owner_id = p_owner` is what keeps one account from ever
-- learning that another holds the same address. People are owner-only,
-- with no admin bypass.
CREATE OR REPLACE FUNCTION person_email_collisions(
    p_owner TEXT, p_self UUID, p_old JSONB, p_new JSONB)
RETURNS TABLE (person_id UUID, email TEXT)
LANGUAGE sql STABLE
AS $$
    SELECT p.id, e.value ->> 'value'
    FROM people p
    CROSS JOIN LATERAL jsonb_array_elements(p.emails) AS e(value)
    WHERE p.owner_id = p_owner
      AND p.id IS DISTINCT FROM p_self
      AND person_email_key(e.value ->> 'value') IN (
          SELECT person_email_key(n.value ->> 'value')
          FROM jsonb_array_elements(p_new) AS n(value)
          WHERE person_email_key(n.value ->> 'value') <> ''
          EXCEPT
          SELECT person_email_key(o.value ->> 'value')
          FROM jsonb_array_elements(p_old) AS o(value)
      )
$$;

CREATE OR REPLACE FUNCTION people_email_identity() RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    had JSONB := '[]'::jsonb;
BEGIN
    -- A restore brings back what its archive holds, pairs included, and
    -- sets this for the statements that insert people, in its own
    -- transaction (SET LOCAL). Nothing else sets it.
    IF current_setting('ember.restoring_people', true) = 'on' THEN
        RETURN NULL;
    END IF;
    -- An address the person already had is not new: a person who shares
    -- one from before can still be edited. A person moved to another
    -- account (nothing does that today) has every address new.
    IF TG_OP = 'UPDATE' AND NEW.owner_id = OLD.owner_id THEN
        had := OLD.emails;
    END IF;
    -- Nothing new: no lock, no check.
    IF NOT EXISTS (
        SELECT person_email_key(n.value ->> 'value')
        FROM jsonb_array_elements(NEW.emails) AS n(value)
        WHERE person_email_key(n.value ->> 'value') <> ''
        EXCEPT
        SELECT person_email_key(o.value ->> 'value')
        FROM jsonb_array_elements(had) AS o(value)
    ) THEN
        RETURN NULL;
    END IF;
    -- The check below must see what a save that held the lock before
    -- this one committed. Only READ COMMITTED gives a statement a fresh
    -- snapshot; under anything else two saves could both pass. Refuse.
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'people are written under read committed'
            USING ERRCODE = 'feature_not_supported';
    END IF;
    -- One writer of new addresses per account at a time. Taken after the
    -- row is written, by every writer in that order.
    PERFORM pg_advisory_xact_lock(hashtext('ember.person_emails.' || NEW.owner_id));
    IF EXISTS (
        SELECT 1 FROM person_email_collisions(NEW.owner_id, NEW.id, had, NEW.emails)
    ) THEN
        -- SECURITY: a fixed message and no DETAIL: no address, and no
        -- person, reaches a log through this error.
        RAISE EXCEPTION 'an email address already belongs to another person'
            USING ERRCODE = 'unique_violation', CONSTRAINT = 'people_email_identity';
    END IF;
    RETURN NULL;
END;
$$;

-- On every update, not only `UPDATE OF emails`: a column list looks at the
-- statement's SET clause, so a BEFORE trigger that changed `emails` in an
-- update of something else would pass unseen. An update that adds no
-- address returns at the first check, before any lock.
DROP TRIGGER IF EXISTS people_email_identity ON people;
CREATE TRIGGER people_email_identity
    AFTER INSERT OR UPDATE ON people
    FOR EACH ROW EXECUTE FUNCTION people_email_identity();
