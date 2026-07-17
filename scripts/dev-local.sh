#!/usr/bin/env bash
# Zero-AWS local dev stack: the base compose + a Keycloak OIDC issuer.
#
#   ./scripts/dev-local.sh            # up --build (foreground)
#   ./scripts/dev-local.sh down -v    # tear down (any args are passed through)
#
# Login at http://localhost:8003 with the seeded users
# (deploy/keycloak/realm-ember-trove.json — dev-only, NOT secrets):
#   admin / admin-devpassword     user / user-devpassword
# Keycloak admin console: http://localhost:8081 (admin / admin-devpassword).
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
# Docker Desktop CLI location on macOS dev machines.
export PATH="$PATH:/Applications/Docker.app/Contents/Resources/bin"

# The base compose requires COOKIE_KEY at interpolation time. Deterministic,
# throwaway, NOT a secret — local http dev only (same approach as e2e.sh).
# Must be exactly 128 HEX chars (the api validates length AND hex-decodes it).
export COOKIE_KEY="${COOKIE_KEY:-$(python3 -c "print('0123456789abcdef'*8)")}"

COMPOSE=(docker compose
  -f deploy/docker-compose.yml -f deploy/docker-compose.local-auth.yml)

if [ "$#" -eq 0 ]; then
  exec "${COMPOSE[@]}" up --build
else
  exec "${COMPOSE[@]}" "$@"
fi
