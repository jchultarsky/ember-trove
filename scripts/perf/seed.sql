-- Synthetic, prod-shaped data for performance measurement (P0, 2026-11-02).
--
-- LOCAL STACKS ONLY: it writes straight into the database, for the e2e test
-- identities. Never run it against prod. No prod data is copied: shapes and
-- sizes come from prod's aggregate counts (2026-11-02):
--   ~300 nodes, 280 tasks, 500 notes, 2,100 versions, 130 edges, 270
--   permission rows, 46 favorites, 940 activity entries. Prod's node bodies
--   average 1.4 KB (max 18 KB); these are 3-6 KB, so body bytes here run
--   about 3x prod's, and edges about 1.4x (183 against 130).
-- `scale` multiplies all of it: 1 = prod today, 10 = growth headroom.
--
--   scripts/perf/seed.sh 10      (wraps this; also cleans up first)
--
-- Every row it makes is findable: node titles start with "perf ", tags with
-- "perf-". Deleting those nodes cascades to everything else (see seed.sh).

\set ON_ERROR_STOP on
SELECT setseed(0.42);

BEGIN;

CREATE TEMP TABLE perf_nodes ON COMMIT DROP AS
SELECT
    gen_random_uuid() AS id,
    i,
    (CASE
        WHEN i % 20 = 0 THEN 'area'
        WHEN i % 10 IN (1, 4, 7) THEN 'project'
        WHEN i % 10 IN (2, 9) THEN 'reference'
        ELSE 'article'
    END)::node_type AS node_type
FROM generate_series(1, 300 * :scale) AS i;

-- Bodies of about 3-6 KB: prose, a heading, a list and a few wiki links.
INSERT INTO nodes (id, owner_id, node_type, title, slug, body, status, project_status,
                   project_state, created_at, updated_at)
SELECT
    n.id,
    'e2e-test-user',
    n.node_type,
    'perf ' || n.node_type || ' ' || n.i,
    'perf-' || n.i || '-' || substr(md5(n.id::text), 1, 8),
    '# Notes on ' || n.i || E'\n\n'
      || repeat('The quick ember drifts over quiet stones while the trove keeps its own counsel. ', 30 + (n.i % 40))
      || E'\n\n- see [[perf article ' || (1 + (n.i * 7) % (300 * :scale)) || ']]'
      || E'\n- and [[perf project ' || (1 + (n.i * 13) % (300 * :scale)) || ']]'
      || E'\n\n' || repeat('Every capture becomes exactly one thing in the end. ', 10),
    (CASE WHEN n.i % 7 = 0 THEN 'published' ELSE 'draft' END)::node_status,
    CASE WHEN n.node_type = 'project' THEN (ARRAY['Active', 'Blocked', 'Done'])[1 + n.i % 3] END,
    -- A Project always has a state (migration 051): the one its label
    -- says, as the migration gave the live rows.
    CASE WHEN n.node_type = 'project'
         THEN (ARRAY['active', 'active', 'complete'])[1 + n.i % 3] END,
    now() - (n.i || ' hours')::interval,
    now() - ((n.i % 500) || ' minutes')::interval
FROM perf_nodes n;

-- The owner's own permission row, as every create writes it.
INSERT INTO permissions (node_id, subject_id, role, granted_by)
SELECT id, 'e2e-test-user', 'owner', 'e2e-test-user' FROM perf_nodes;

-- About a tenth shared with the second identity (viewer or editor).
INSERT INTO permissions (node_id, subject_id, role, granted_by)
SELECT id, 'e2e-other-user',
       (CASE WHEN i % 2 = 0 THEN 'viewer' ELSE 'editor' END)::permission_role,
       'e2e-test-user'
FROM perf_nodes WHERE i % 10 = 3;

-- Homes: projects and articles under an area (one home each).
INSERT INTO edges (source_id, target_id, edge_type)
SELECT a.id, c.id, 'contains'
FROM perf_nodes c
JOIN perf_nodes a ON a.node_type = 'area' AND a.i = 20 * (1 + (c.i % (15 * :scale)))
WHERE c.node_type IN ('project', 'article') AND c.i % 3 = 0;

-- References and related links (related_to once per pair: source < target).
INSERT INTO edges (source_id, target_id, edge_type)
SELECT s.id, t.id, 'references'
FROM perf_nodes s
JOIN perf_nodes t ON t.i = 1 + (s.i * 31) % (300 * :scale)
WHERE s.i % 4 = 0 AND s.id <> t.id
ON CONFLICT DO NOTHING;
INSERT INTO edges (source_id, target_id, edge_type)
SELECT LEAST(s.id, t.id), GREATEST(s.id, t.id), 'related_to'
FROM perf_nodes s
JOIN perf_nodes t ON t.i = 1 + (s.i * 17) % (300 * :scale)
WHERE s.i % 9 = 0 AND s.id <> t.id
ON CONFLICT DO NOTHING;

-- Tasks: about one per node, all on Projects (only Projects take tasks).
INSERT INTO node_tasks (node_id, owner_id, title, status, priority, focus_date, due_date, sort_order)
SELECT p.id, 'e2e-test-user',
       'perf task ' || p.i || '-' || k,
       (ARRAY['open', 'open', 'open', 'done', 'cancelled'])[1 + (p.i + k) % 5]::task_status,
       (ARRAY['low', 'medium', 'high'])[1 + (p.i + k) % 3]::task_priority,
       CASE WHEN (p.i + k) % 11 = 0 THEN current_date END,
       CASE WHEN (p.i + k) % 4 = 0 THEN current_date + ((p.i + k) % 40 - 10) END,
       k * 10
FROM perf_nodes p, generate_series(1, 3) AS k
WHERE p.node_type = 'project';

-- Notes: about 1.7 per node.
INSERT INTO node_notes (node_id, owner_id, body, created_at)
SELECT n.id, 'e2e-test-user',
       'perf note ' || n.i || '-' || k || ': ' || repeat('a passing thought worth keeping ', 4 + k),
       now() - ((n.i * k) || ' minutes')::interval
FROM perf_nodes n, generate_series(1, 2) AS k
WHERE (n.i + k) % 6 <> 0;

-- Versions: 7 per node, full bodies (that is what autosave records).
INSERT INTO node_versions (node_id, body, created_by, created_at)
SELECT n.id, nd.body || E'\n\nrevision ' || k, 'e2e-test-user', now() - ((n.i + k) || ' hours')::interval
FROM perf_nodes n JOIN nodes nd ON nd.id = n.id, generate_series(1, 7) AS k;

-- Tags: 50 per scale, about 1.5 per node.
CREATE TEMP TABLE perf_tags ON COMMIT DROP AS
SELECT gen_random_uuid() AS id, j FROM generate_series(1, 50 * :scale) AS j;
INSERT INTO tags (id, owner_id, name)
SELECT id, 'e2e-test-user', 'perf-' || j FROM perf_tags;
INSERT INTO node_tags (node_id, tag_id)
SELECT n.id, t.id FROM perf_nodes n JOIN perf_tags t ON t.j = 1 + n.i % (50 * :scale)
UNION
SELECT n.id, t.id FROM perf_nodes n JOIN perf_tags t ON t.j = 1 + (n.i * 3) % (50 * :scale)
WHERE n.i % 2 = 0;

-- Favorites: 46 per scale.
INSERT INTO user_favorites (owner_id, node_id, label, position)
SELECT 'e2e-test-user', id, 'perf fav ' || i, i FROM perf_nodes WHERE i <= 46 * :scale;

-- Activity: about 3 per node.
INSERT INTO activity_log (node_id, subject_id, action, created_at)
SELECT n.id, 'e2e-test-user', (ARRAY['created', 'edited', 'edited'])[k],
       now() - ((n.i * k) || ' minutes')::interval
FROM perf_nodes n, generate_series(1, 3) AS k;

-- Saved graph positions for every node (the graph view loads them pinned).
INSERT INTO node_positions (node_id, x, y)
SELECT id, (i % 60) * 90.0, (i / 60) * 90.0 FROM perf_nodes;

COMMIT;

ANALYZE;

SELECT (SELECT count(*) FROM nodes WHERE title LIKE 'perf %')      AS nodes,
       (SELECT count(*) FROM edges)                                AS edges,
       (SELECT count(*) FROM node_tasks WHERE title LIKE 'perf %') AS tasks,
       (SELECT count(*) FROM node_notes WHERE body LIKE 'perf %')  AS notes,
       (SELECT count(*) FROM node_versions)                        AS versions,
       pg_size_pretty(pg_database_size(current_database()))        AS db_size;
