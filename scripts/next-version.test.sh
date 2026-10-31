#!/usr/bin/env bash
# Unit tests for next-version.sh's commit classifier (no git).
#   bash scripts/next-version.test.sh
set -euo pipefail

# shellcheck source=next-version.sh
source "$(dirname "$0")/next-version.sh"

FAILS=0
check() { # name, expected, actual
  if [ "$2" = "$3" ]; then
    echo "ok   $1"
  else
    echo "FAIL $1"; echo "  expected: [$2]"; echo "  actual:   [$3]"
    FAILS=$((FAILS + 1))
  fi
}
bump() { printf '%s' "$1" | bump_for; }

check "fix"                  patch "$(bump 'fix(ui): a thing')"
check "chore and docs"       patch "$(bump $'chore: x\n\ndocs: y')"
check "feat"                 minor "$(bump 'feat: x')"
check "feat with scope"      minor "$(bump 'feat(api): x')"
check "feat in a merge body" minor "$(bump $'Merge pull request #1 from a/b\n\nfeat(ui): x')"
check "bang"                 major "$(bump 'feat!: x')"
check "bang with scope"      major "$(bump 'feat(ontology)!: Resource merges into Reference')"
check "bang on another type" major "$(bump 'refactor(api)!: x')"
check "footer"               major "$(bump $'fix: x\n\nBREAKING CHANGE: old backups are refused')"
check "footer, hyphenated"   major "$(bump $'fix: x\n\nBREAKING-CHANGE: y')"
check "major wins"           major "$(bump $'feat: a\n\nfix(x)!: b')"
# Not breaking: prose that only mentions the words.
check "prose mention"        patch "$(bump $'fix: x\n\nThis is not a BREAKING CHANGE: see #1')"
check "bang mid-subject"     patch "$(bump 'fix: handle feat!: in titles')"
check "nothing"              patch "$(bump '')"
# Bigger than a pipe buffer, with the match first: grep -q stops early, and
# under pipefail (set above) a piped writer's SIGPIPE used to read as a miss.
BIG=$'feat(x)!: y\n'"$(printf 'filler line %06d\n' $(seq 1 6000))"
check "early match, big input" major "$(bump "$BIG")"

if [ "$FAILS" -gt 0 ]; then
  echo "$FAILS failed"; exit 1
fi
echo "all passed"
