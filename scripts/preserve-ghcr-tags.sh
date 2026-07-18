#!/usr/bin/env bash
# One-time archival: copy every ember-trove image tag that exists under the OLD
# GHCR namespace (jchultarsky101) but NOT yet under the NEW one (jchultarsky),
# so the old packages can be deleted without losing pre-transfer release images.
#
# Registry-to-registry (no pull/push of layers); idempotent (skips tags already
# present in the new namespace); continues past failures and prints a summary,
# so it is safe to re-run if it stops partway.
#
# PREREQUISITES (yours to do — package writes are credential-scoped):
#   1. Create a *classic* PAT on the jchultarsky account with `write:packages`
#      (https://github.com/settings/tokens → Generate new token (classic)),
#      7-day expiry, that one scope only.
#   2. docker login ghcr.io -u jchultarsky   # paste the PAT as the password
#   3. ./preserve-ghcr-tags.sh
#   4. Verify the summary shows 0 failures, then delete the old packages.
#
# Docker Desktop must be running; buildx is bundled with it.
#
# VERSION FILTER (optional): set TAG_FILTER to an extended-regex; only old tags
# matching it are copied (default `.` = everything). The regex is matched with
# bash `=~` (ERE), so `\.` is a literal dot. Examples:
#   TAG_FILTER='^v2\.'              ./preserve-ghcr-tags.sh   # only the 2.x line
#   TAG_FILTER='^v2\.(1[5-9]|2[0-9])\.'  ./…                  # v2.15.0 and newer
#   TAG_FILTER='^v2\.2[12]\.'       ./…                       # just v2.21.x + v2.22.x
# Do a dry run first to see the count without copying:
#   DRY_RUN=1 TAG_FILTER='^v2\.2[12]\.' ./preserve-ghcr-tags.sh
set -uo pipefail
export PATH="$PATH:/Applications/Docker.app/Contents/Resources/bin"

OLD_OWNER="jchultarsky101"
NEW_OWNER="jchultarsky"
IMAGES=("ember-trove-api" "ember-trove-ui")
TAG_FILTER="${TAG_FILTER:-.}"   # ERE; default matches every tag
DRY_RUN="${DRY_RUN:-0}"         # 1 = list what would be copied, do not copy

# Fetch every tag for a public GHCR repo, following pagination.
list_tags() {
  local owner=$1 img=$2
  local tok
  tok=$(curl -s "https://ghcr.io/token?scope=repository:${owner}/${img}:pull" \
        | python3 -c "import sys,json;print(json.load(sys.stdin).get('token',''))")
  python3 - "$owner" "$img" "$tok" <<'PY'
import sys, json, urllib.request
owner, img, tok = sys.argv[1:4]
tags, last = [], None
while True:
    url = f"https://ghcr.io/v2/{owner}/{img}/tags/list?n=100" + (f"&last={last}" if last else "")
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {tok}"})
    with urllib.request.urlopen(req, timeout=30) as r:
        page = json.load(r).get("tags") or []
        link = r.headers.get("Link", "")
    if not page: break
    tags += page
    if 'rel="next"' not in link: break
    last = page[-1]
print("\n".join(tags))
PY
}

# NOTE: written for stock macOS bash 3.2 — no `mapfile`, no associative arrays.
# Set membership is a `grep -qxF` against the newline-delimited new-tag list;
# the main loop uses a here-string (`<<<`) so it does NOT run in a subshell and
# the counters persist.
total_copied=0 total_skipped=0 total_failed=0 total_filtered=0 total_wouldcopy=0
failures=""
[ "$TAG_FILTER" != "." ] && echo "TAG_FILTER = /${TAG_FILTER}/"
[ "$DRY_RUN" = "1" ] && echo "DRY RUN — nothing will be copied"

for img in "${IMAGES[@]}"; do
  echo "══════ ${img} ══════"
  old_tags=$(list_tags "$OLD_OWNER" "$img")
  new_tags=$(list_tags "$NEW_OWNER" "$img")
  old_n=$(printf '%s\n' "$old_tags" | grep -c .)
  new_n=$(printf '%s\n' "$new_tags" | grep -c .)
  echo "  old tags: ${old_n} | already in new: ${new_n}"

  while IFS= read -r tag; do
    [ -z "$tag" ] && continue
    # Version filter: skip tags that don't match TAG_FILTER (unquoted RHS = regex).
    if ! [[ "$tag" =~ $TAG_FILTER ]]; then
      total_filtered=$((total_filtered+1)); continue
    fi
    # Skip if already present in the new namespace.
    if printf '%s\n' "$new_tags" | grep -qxF -- "$tag"; then
      total_skipped=$((total_skipped+1)); continue
    fi
    src="ghcr.io/${OLD_OWNER}/${img}:${tag}"
    dst="ghcr.io/${NEW_OWNER}/${img}:${tag}"
    if [ "$DRY_RUN" = "1" ]; then
      echo "  ○ would copy ${img}:${tag}"
      total_wouldcopy=$((total_wouldcopy+1)); continue
    fi
    if docker buildx imagetools create --tag "$dst" "$src" >/dev/null 2>&1; then
      echo "  ✓ ${img}:${tag}"
      total_copied=$((total_copied+1))
    else
      echo "  ✗ ${img}:${tag}  (FAILED)"
      failures="${failures}    - ${img}:${tag}"$'\n'
      total_failed=$((total_failed+1))
    fi
  done <<< "$old_tags"
done

echo
echo "══════ SUMMARY ══════"
if [ "$DRY_RUN" = "1" ]; then
  echo "  would copy: $total_wouldcopy"
  echo "  skipped:    $total_skipped (already in new namespace)"
  echo "  filtered:   $total_filtered (excluded by TAG_FILTER)"
  echo "  (dry run — re-run without DRY_RUN=1 to perform the copy)"
  exit 0
fi
echo "  copied:   $total_copied"
echo "  skipped:  $total_skipped (already in new namespace)"
echo "  filtered: $total_filtered (excluded by TAG_FILTER)"
echo "  failed:   $total_failed"
if [ "$total_failed" -gt 0 ]; then
  printf '%s' "$failures"
  echo "  Re-run the script to retry failures (copied tags are skipped)."
  exit 1
fi
if [ "$total_filtered" -gt 0 ]; then
  echo "  Matched tags preserved — but ${total_filtered} tag(s) were EXCLUDED by"
  echo "  TAG_FILTER and still exist ONLY under ${OLD_OWNER}. Deleting the old"
  echo "  packages will permanently lose those. Widen TAG_FILTER first if you"
  echo "  want them too."
else
  echo "  All tags preserved. Safe to delete the old ${OLD_OWNER} packages."
fi
