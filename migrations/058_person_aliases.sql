-- S8c (#363; ONTOLOGY §7.1, §7.3): the id of a person who was merged
-- into another.
--
-- A merge keeps one person and deletes the other. Texts mention people
-- by id (`@[label](person:<id>)`), in bodies, notes and old versions, on
-- nodes of this account and of others. The merge changes none of that
-- text (owner decision, 2026-11-19). It records here that the id of the
-- person who went now stands for the one kept, and the API reads a
-- mention token through this table (`api/src/repo/mention.rs`).
--
-- Always one step: `person_id` is a living person (the foreign key) and
-- `old_id` never is (the person was deleted in the merge's transaction,
-- and ids are not reused). A later merge of the kept person into a third
-- re-points the rows; there is no chain to follow. The trigger below
-- refuses an `old_id` that is a person, so a mistake cannot make one
-- living person's id stand for another's.
--
-- The cascade: a deleted person takes the aliases that pointed to them,
-- and those tokens then name nobody, as the person's own do.
--
-- No account column: whose an alias is follows from `people.owner_id`,
-- as for `node_mentions` (057). An account's purge and a restore reach
-- it through the cascade from `people`.
--
-- This migration writes no data and changes no row.

CREATE TABLE person_aliases (
    old_id    UUID PRIMARY KEY,
    person_id UUID NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    merged_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT person_aliases_not_itself CHECK (old_id <> person_id)
);

-- "Whose ids stand for this person", and the cascade.
CREATE INDEX person_aliases_person_idx ON person_aliases (person_id);

CREATE FUNCTION person_alias_is_of_someone_gone() RETURNS trigger AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM people WHERE id = NEW.old_id) THEN
        RAISE EXCEPTION 'an alias must be the id of a person who is gone'
            USING ERRCODE = 'check_violation',
                  CONSTRAINT = 'person_aliases_gone';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER person_aliases_gone
    BEFORE INSERT OR UPDATE OF old_id ON person_aliases
    FOR EACH ROW EXECUTE FUNCTION person_alias_is_of_someone_gone();
