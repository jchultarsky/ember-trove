-- S9 (#360; ONTOLOGY §7.3, §14): mentions of People in text.
--
-- A text mentions a person as `@[label](person:<id>)`. These two tables
-- are the record of which mentions are REAL: made by an account that
-- saved the text, of one of its own people. The API writes a row only
-- then (`api/src/repo/mention.rs`), in the transaction that writes the
-- text. A token with no row is plain text that looks like a mention:
-- nobody is named for it. That is what makes a person's id, which a
-- sharee can see, useless to them.
--
-- Two tables, not one with a nullable column: a body's rows and a
-- note's must never be read for each other (a co-editor could otherwise
-- turn a mention in someone's note into one in the body). A note's node
-- is always read from `node_notes`, never from a second copy here.
--
-- Both ends cascade. A deleted person takes their rows, so what is left
-- in the text shows as unresolved. A note in the trash keeps its rows
-- (it can be restored) and every read leaves them out.
--
-- No account column: whose a row is follows from `people.owner_id`.
--
-- This migration writes no data and changes no row.

CREATE TABLE node_mentions (
    node_id   UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    person_id UUID NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    PRIMARY KEY (node_id, person_id)
);

-- "Mentioned in", and the cascade from a deleted person.
CREATE INDEX node_mentions_person_idx ON node_mentions (person_id);

CREATE TABLE note_mentions (
    note_id   UUID NOT NULL REFERENCES node_notes(id) ON DELETE CASCADE,
    person_id UUID NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    PRIMARY KEY (note_id, person_id)
);

CREATE INDEX note_mentions_person_idx ON note_mentions (person_id);
