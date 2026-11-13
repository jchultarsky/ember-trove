#!/usr/bin/env bash
# Playwright smoke suite orchestrator — see e2e/README.md.
#
# Brings up the dedicated e2e Docker stack (auth bypassed, ephemeral DB,
# compose project `ember-e2e` so the dev stack is untouched), runs the suite,
# tears the stack down.
#
#   ./scripts/e2e.sh                 # full run (Playwright in Docker)
#   KEEP_STACK=1 ./scripts/e2e.sh    # leave the stack up for iteration
#   E2E_RUNNER=native ./scripts/e2e.sh   # use a local node + installed browsers
#
# A second stack beside one already running (another session's, or a kept
# one): give it its own compose project and UI port. Its database, S3 and API
# get no host ports at all, so nothing collides.
#
#   E2E_PROJECT=ember-e2e-b E2E_PORT=18003 ./scripts/e2e.sh
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
# Docker Desktop CLI location on macOS dev machines.
export PATH="$PATH:/Applications/Docker.app/Contents/Resources/bin"

# Must match the @playwright/test version in e2e/package.json.
PLAYWRIGHT_IMAGE="mcr.microsoft.com/playwright:v1.57.0-noble"

# The base compose requires COOKIE_KEY at interpolation time (before the
# override merges). Throwaway, deterministic, NOT a secret — the bypass
# never issues cookies; config just validates the key's shape.
export COOKIE_KEY="${COOKIE_KEY:-$(python3 -c "print(('e2e0'*16)+('0123456789abcdef'*4))")}"

E2E_PROJECT="${E2E_PROJECT:-ember-e2e}"
E2E_PORT="${E2E_PORT:-8003}"
# Both go into a compose file and a URL: take nothing but a plain name and a
# port number.
if ! [[ "$E2E_PROJECT" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
  echo "✗ E2E_PROJECT must be a compose project name (a-z, 0-9, - and _)" >&2
  exit 2
fi
if ! [[ "$E2E_PORT" =~ ^[0-9]{2,5}$ ]]; then
  echo "✗ E2E_PORT must be a port number" >&2
  exit 2
fi
if [ "$E2E_PROJECT" != "ember-e2e" ] && [ "$E2E_PORT" = "8003" ]; then
  echo "✗ a second stack needs its own port too: set E2E_PORT (for example 18003)" >&2
  exit 2
fi

COMPOSE=(docker compose -p "$E2E_PROJECT"
  -f deploy/docker-compose.yml -f deploy/docker-compose.e2e.yml)
if [ "$E2E_PORT" != "8003" ]; then
  # Moves the UI to E2E_PORT and takes the other services off the host.
  export E2E_PORT
  COMPOSE+=(-f deploy/docker-compose.e2e-ports.yml)
fi
APP="http://localhost:${E2E_PORT}"

cleanup() {
  if [ "${KEEP_STACK:-0}" != "1" ]; then
    echo "── Tearing down the e2e stack (KEEP_STACK=1 to skip)…"
    "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

echo "── Building & starting the e2e stack…"
"${COMPOSE[@]}" up -d --build

echo "── Waiting for the app to become healthy…"
healthy=0
for _ in $(seq 1 90); do
  if curl -fs "$APP/api/health" >/dev/null 2>&1; then
    healthy=1
    break
  fi
  sleep 2
done
if [ "$healthy" != "1" ]; then
  echo "✗ stack failed to become healthy; api logs:" >&2
  "${COMPOSE[@]}" logs api | tail -50 >&2
  exit 1
fi

# Sanity check: the auth bypass must be active (an unauthenticated /auth/me
# succeeds only with the bypass armed). Refuse to run otherwise — the suite
# would just produce 40 confusing login redirects.
if ! curl -fs "$APP/api/auth/me" >/dev/null 2>&1; then
  echo "✗ E2E auth bypass is not active (did the api image build with --features e2e-bypass?)" >&2
  exit 1
fi

echo "── Running Playwright…"
if [ "${E2E_RUNNER:-docker}" = "native" ]; then
  (cd e2e && npm ci && E2E_BASE_URL="${E2E_BASE_URL:-$APP}" npx playwright test "$@")
else
  # No local Node required: run the suite inside the official Playwright
  # image (browsers preinstalled). host-gateway makes the host's :8003
  # reachable from the container on both macOS and Linux.
  docker run --rm \
    --add-host=host.docker.internal:host-gateway \
    -v "$REPO/e2e":/work -w /work \
    -e E2E_BASE_URL="http://host.docker.internal:${E2E_PORT}" \
    -e CI="${CI:-}" \
    "$PLAYWRIGHT_IMAGE" \
    bash -lc "npm ci && npx playwright test $*"
fi

echo "✓ e2e suite passed"
