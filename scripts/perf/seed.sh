#!/usr/bin/env bash
# Load (or remove) synthetic, prod-shaped data into the LOCAL e2e stack for
# performance measurement (P0, 2026-11-02). Never points at prod: it only
# looks for a local container named *e2e*postgres*.
#
#   KEEP_STACK=1 ./scripts/e2e.sh tests/smoke.spec.ts   # bring the stack up
#   scripts/perf/seed.sh 1        # prod-sized
#   scripts/perf/seed.sh 10       # ten times prod
#   scripts/perf/seed.sh clean    # remove it again
set -euo pipefail
export PATH="$PATH:/Applications/Docker.app/Contents/Resources/bin"

HERE="$(cd "$(dirname "$0")" && pwd)"
PG="$(docker ps --format '{{.Names}}' | grep -E 'e2e.*postgres' | head -1)"
if [ -z "$PG" ]; then
  echo "no local e2e postgres container running (start the stack with KEEP_STACK=1 ./scripts/e2e.sh)" >&2
  exit 1
fi
psql_() { docker exec -i "$PG" sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -q -v ON_ERROR_STOP=1 "$@"' sh "$@"; }

# Cascades from the nodes take edges, tasks, notes, versions, tags on nodes,
# favorites, activity and positions with them.
psql_ <<'SQL'
DELETE FROM nodes WHERE title LIKE 'perf %' AND owner_id = 'e2e-test-user';
DELETE FROM tags  WHERE name  LIKE 'perf-%' AND owner_id = 'e2e-test-user';
SQL
[ "${1:-}" = "clean" ] && { echo "perf data removed"; exit 0; }

SCALE="${1:-1}"
case "$SCALE" in ''|*[!0-9]*) echo "scale must be a number or 'clean'" >&2; exit 1 ;; esac
psql_ -v scale="$SCALE" < "$HERE/seed.sql"
