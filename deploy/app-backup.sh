#!/usr/bin/env bash
# deploy/app-backup.sh — make an automatic app backup on the production host.
#
#   ~/ember-trove/deploy/app-backup.sh "Daily"
#
# Runs the API's own command (`ember-trove-api backup create`) inside the
# running container: the same backup as Admin → Backup → Create, listed with
# an Automatic badge, newest 10 automatic backups kept. Called over SSH by
# the release workflow after each deploy and by the Daily Backup workflow.
#
# Callers' logs can be public (the repo is public), so this prints only the
# two summary lines (backup id and counts). The command's full output goes
# to ~/ember-trove-backups.log, owner-only. Exits non-zero when the backup
# fails, so the calling workflow run turns red and GitHub emails the owner.
set -euo pipefail
umask 077

COMMENT="${1:?usage: app-backup.sh COMMENT}"
LOG="$HOME/ember-trove-backups.log"

OUT=$(docker exec deploy-api-1 ember-trove-api backup create --comment "$COMMENT" 2>&1) \
  && STATUS=0 || STATUS=$?
{ echo "=== $(date -u +%FT%TZ) ${COMMENT} exit ${STATUS}"; echo "$OUT"; } >> "$LOG"
echo "$OUT" | grep -E '^(backup [0-9a-f-]+ created:|deleted [0-9]+ automatic)' || true

if [ "$STATUS" -ne 0 ]; then
  echo "Backup failed (exit ${STATUS}); details in ~/ember-trove-backups.log on the server."
  exit 1
fi
