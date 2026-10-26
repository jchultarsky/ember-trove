-- #147: People. A registry of persons (an entity, not a node type: nodes
-- are documents, and a person is someone documents refer to), plus
-- memberships that place a person in a node's context.
--
-- PRIVACY: these are third parties' contact details. A person row is only
-- ever read for its owner_id (no admin bypass, enforced in the repo and
-- routes). Node sharees see a membership's name and role only. The CHECKs
-- mirror `common::person` limits as defence in depth.

CREATE TABLE people (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      TEXT        NOT NULL,
    display_name  TEXT        NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 200),
    company       TEXT        NULL     CHECK (company IS NULL OR char_length(company) <= 200),
    position      TEXT        NULL     CHECK (position IS NULL OR char_length(position) <= 200),
    -- [{"label": "work", "value": "...", "preferred": true}, ...]
    emails        JSONB       NOT NULL DEFAULT '[]'
                              CHECK (jsonb_typeof(emails) = 'array' AND jsonb_array_length(emails) <= 10),
    phones        JSONB       NOT NULL DEFAULT '[]'
                              CHECK (jsonb_typeof(phones) = 'array' AND jsonb_array_length(phones) <= 10),
    address_label TEXT        NULL     CHECK (address_label IS NULL OR char_length(address_label) <= 30),
    address       TEXT        NULL     CHECK (address IS NULL OR char_length(address) <= 1000),
    notes         TEXT        NULL     CHECK (notes IS NULL OR char_length(notes) <= 10000),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX people_owner_name_idx ON people (owner_id, lower(display_name));

CREATE TRIGGER people_updated_at
    BEFORE UPDATE ON people
    FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- A membership: one per person per node. Deleting the node or the person
-- removes it; removing a membership never deletes the person.
CREATE TABLE node_people (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    node_id     UUID        NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    person_id   UUID        NOT NULL REFERENCES people(id) ON DELETE CASCADE,
    role        TEXT        NULL CHECK (role IS NULL OR char_length(role) <= 100),
    note        TEXT        NULL CHECK (note IS NULL OR char_length(note) <= 2000),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT node_people_unique UNIQUE (node_id, person_id)
);

CREATE INDEX node_people_person_idx ON node_people (person_id);
