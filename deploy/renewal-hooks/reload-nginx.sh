#!/bin/bash
# Certbot deploy hook: reload nginx inside the Docker proxy container after a
# successful certificate renewal so it picks up the new cert (the cert dir is
# mounted read-only and nginx caches certs in memory).
#
# Certbot runs this as root; the Docker socket is accessible. We talk to Docker
# directly (no `docker compose` + env file) so renewal never depends on the
# app's secrets being present in the shell.
set -euo pipefail

PROXY="$(docker ps --filter name=proxy --filter status=running --format '{{.Names}}' | head -n1)"

if [ -n "$PROXY" ]; then
    docker exec "$PROXY" nginx -s reload
    echo "[certbot-deploy] reloaded nginx in $PROXY"
else
    echo "[certbot-deploy] no running proxy container found — skipping reload" >&2
fi
