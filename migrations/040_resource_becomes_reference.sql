-- #222 (ontology S2c, docs/ONTOLOGY.md §3.1): the legacy Resource node kind
-- merges into Reference. PARA and GTD each kept one of the two words for the
-- same thing; Trove had both.
--
-- Postgres can't drop a value from an enum, so 'resource' stays in the
-- node_type type and a CHECK keeps it out of the table instead. Templates
-- store their kind as TEXT and get the same treatment.

-- Templates first. Each user has at most one default template per kind
-- (node_templates_default_uidx). A user with defaults for both kinds keeps
-- the Reference one; the Resource template stays, as a non-default
-- Reference template, so nothing they wrote is lost.
UPDATE node_templates t
   SET is_default = FALSE
 WHERE t.node_type = 'resource'
   AND t.is_default
   AND EXISTS (SELECT 1 FROM node_templates r
                WHERE r.created_by = t.created_by
                  AND r.node_type = 'reference'
                  AND r.is_default);
UPDATE node_templates SET node_type = 'reference' WHERE node_type = 'resource';
ALTER TABLE node_templates
  ADD CONSTRAINT node_templates_no_resource CHECK (node_type <> 'resource');

-- A change of kind isn't an edit: keep updated_at, so an editor open on one
-- of these nodes still saves against the version it loaded (#161).
SET LOCAL ember.keep_updated_at = 'on';
UPDATE nodes SET node_type = 'reference' WHERE node_type = 'resource';
ALTER TABLE nodes
  ADD CONSTRAINT nodes_no_resource CHECK (node_type <> 'resource');
