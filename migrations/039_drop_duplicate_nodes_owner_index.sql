-- Migration 039: drop the duplicate index on nodes.owner_id
--
-- Migration 001 already creates `idx_nodes_owner ON nodes(owner_id)`; 021
-- added `nodes_owner_id_idx` on the same column, so every node write paid
-- for two identical btrees. `idx_nodes_owner` stays and still serves every
-- owner-scoped lookup, so query plans are unchanged.
--
-- Not CONCURRENTLY: sqlx runs each migration in a transaction, and the
-- brief lock at startup is fine for a table this size.

DROP INDEX IF EXISTS nodes_owner_id_idx;
