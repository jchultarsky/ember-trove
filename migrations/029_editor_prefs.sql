-- Per-item editor UI preferences (currently just resized editor height),
-- kept separate from node_tasks / node_notes so adding UI prefs never touches
-- the core entity queries. Keyed by owner + entity kind + entity id.
CREATE TABLE IF NOT EXISTS editor_prefs (
    owner_id    TEXT    NOT NULL,
    entity_kind TEXT    NOT NULL CHECK (entity_kind IN ('task', 'note')),
    entity_id   UUID    NOT NULL,
    height      INTEGER NOT NULL,
    PRIMARY KEY (owner_id, entity_kind, entity_id)
);
