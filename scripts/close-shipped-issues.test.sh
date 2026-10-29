#!/usr/bin/env bash
# Unit tests for close-shipped-issues.sh's pure parts (no git, no network).
#   bash scripts/close-shipped-issues.test.sh
set -euo pipefail

# shellcheck source=close-shipped-issues.sh
source "$(dirname "$0")/close-shipped-issues.sh"

FAILS=0
check() { # name, expected, actual
  if [ "$2" = "$3" ]; then
    echo "ok   $1"
  else
    echo "FAIL $1"; echo "  expected: [$2]"; echo "  actual:   [$3]"
    FAILS=$((FAILS + 1))
  fi
}
refs() { printf '%s' "$1" | closing_refs | tr '\n' ' ' | sed 's/ $//'; }

# Closing keywords, the forms GitHub accepts.
check "fixes"            "190"        "$(refs 'Fixes #190')"
check "every keyword"    "1 2 3 4 5 6 7 8 9" \
  "$(refs 'close #1, closes #2, closed #3, fix #4, fixes #5, fixed #6, resolve #7, resolves #8, resolved #9')"
check "case and colon"   "12 13"      "$(refs 'FIXES: #12 and Closes:#13')"
check "multi-line body"  "183 184"    "$(refs $'What and why.\n\nFixes #183\nFixes #184\n')"
check "deduplicated"     "5"          "$(refs 'Fixes #5. Also fixes #5.')"

# Not closing: a partial fix says Refs, and the rest is ordinary prose.
check "refs stays open"  ""           "$(refs 'Refs #184')"
check "plain mention"    ""           "$(refs 'Follow-up to #184 and PR #193.')"
check "word containing a keyword" "" "$(refs 'prefixes #3, unresolved #4, suffixed #5')"
check "other repo"       ""           "$(refs 'Fixes jchultarsky/mirador#255')"
check "no number"        ""           "$(refs 'Fixes the bug')"

# The previous release by version order, not by ancestry (Git Flow).
TAGS=$'v2.9.0\nv2.10.0\nv2.44.0\nv2.44.1\nv2.43.1'
check "previous tag"         "v2.44.0" "$(printf '%s\n' "$TAGS" | prev_tag_from_list v2.44.1)"
check "previous across minor" "v2.43.1" "$(printf '%s\n' "$TAGS" | prev_tag_from_list v2.44.0)"
check "numeric, not lexical"  "v2.9.0"  "$(printf '%s\n' "$TAGS" | prev_tag_from_list v2.10.0)"
check "first release"         ""        "$(printf '%s\n' "$TAGS" | prev_tag_from_list v2.9.0)"

# PR numbers from GitHub's merge-commit subjects.
SUBJECTS=$'Merge pull request #194 from jchultarsky/feature/jc/issue-184-name-autofill\nchore: merge release/2.44.1 into main\nMerge pull request #192 from jchultarsky/x\nMerge branch \'develop\''
check "pr numbers" "194 192" "$(printf '%s\n' "$SUBJECTS" | pr_numbers | tr '\n' ' ' | sed 's/ $//')"

if [ "$FAILS" -gt 0 ]; then
  echo "$FAILS failed"; exit 1
fi
echo "all passed"
