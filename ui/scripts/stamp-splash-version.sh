#!/bin/sh
# Trunk post_build hook: stamp the release version into the loading screen.
#
# The splash (index.html, #etl-version) must name the version of THIS bundle,
# not the server's: the service worker serves the app shell cache-first, so a
# slow or stuck load is often an old cached bundle talking to a newer API
# (#120). The source of truth is api/Cargo.toml's package version — the one
# every release bumps and /api/health reports — so this needs no release step.
#
# Fails the build if the version is missing or malformed (it is written into
# HTML, so only a plain semver is accepted) or a placeholder survives.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
manifest="$here/../../api/Cargo.toml"
html="${TRUNK_STAGING_DIR:?run as a Trunk post_build hook}/index.html"

version="$(awk -F'"' '/^version[ \t]*=/ { print $2; exit }' "$manifest")"
case "$version" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "stamp-splash-version: bad version '$version' in $manifest" >&2; exit 1 ;;
esac
case "$version" in
  *[!0-9A-Za-z.+-]*) echo "stamp-splash-version: unsafe version '$version'" >&2; exit 1 ;;
esac

tmp="$html.stamp"
sed "s/%%APP_VERSION%%/$version/g" "$html" > "$tmp"
mv "$tmp" "$html"

if awk '/%%APP_VERSION%%/ { found = 1 } END { exit !found }' "$html"; then
  echo "stamp-splash-version: placeholder left in $html" >&2
  exit 1
fi
echo "stamp-splash-version: splash stamped v$version"
