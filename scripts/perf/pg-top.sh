#!/usr/bin/env bash
# The slowest statements on a database, from pg_stat_statements (performance
# phase P0, 2026-11-02). Read-only. Statements are normalised by Postgres
# ($1, $2 …), so no values, and no personal data, are printed.
#
#   scripts/perf/pg-top.sh            # prod, over `ssh trove`
#   scripts/perf/pg-top.sh --local    # the local/e2e stack
#   LIMIT=30 scripts/perf/pg-top.sh   # more rows
#
# One-time setup per database (the server must preload the library, see
# deploy/docker-compose*.yml):  CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
# Reset the counters for a fresh window:  SELECT pg_stat_statements_reset();
set -euo pipefail

LIMIT="${LIMIT:-15}"
SQL="
SELECT round(total_exec_time::numeric, 1)        AS total_ms,
       calls,
       round(mean_exec_time::numeric, 2)         AS mean_ms,
       round(max_exec_time::numeric, 1)          AS max_ms,
       rows,
       shared_blks_read                          AS blks_read,
       left(regexp_replace(query, '\s+', ' ', 'g'), 160) AS query
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
ORDER BY total_exec_time DESC
LIMIT ${LIMIT};"

psql_in() { # $1 = container name
  docker exec -i "$1" sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -P pager=off' <<<"$SQL"
}

if [ "${1:-}" = "--local" ]; then
  export PATH="$PATH:/Applications/Docker.app/Contents/Resources/bin"
  psql_in "$(docker ps --format '{{.Names}}' | grep -m1 postgres)"
else
  ssh trove "docker exec -i deploy-postgres-1 sh -c 'psql -U \"\$POSTGRES_USER\" -d \"\$POSTGRES_DB\" -P pager=off'" <<<"$SQL"
fi
