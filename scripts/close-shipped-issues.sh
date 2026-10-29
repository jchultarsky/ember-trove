#!/usr/bin/env bash
# Closes the issues a release shipped (run by release.yml after the GitHub
# Release is created).
#
#   ./scripts/close-shipped-issues.sh v2.44.1            # comment + close
#   ./scripts/close-shipped-issues.sh v2.44.1 --dry-run  # only list them
#
# PRs merge into develop, and GitHub auto-closes an issue only on a merge
# into the default branch (main), so shipped issues stayed open until closed
# by hand; #183 was missed after v2.44.0 (2026-10-29).
#
# Which issues: the PRs merged between the previous tag and this one, and in
# each PR body the issues named with a closing keyword (`Fixes #n`,
# `Closes #n`, `Resolves #n`). `Refs #n` marks a partial fix (e.g. one still
# waiting for a manual re-test) and leaves the issue open. Each open issue
# gets the "Shipped in vX.Y.Z." comment and is closed as completed; closed
# issues and PR numbers are skipped, so a rerun is harmless.
#
# Untrusted input: PR bodies are only scanned for `#<digits>`; nothing from
# them is evaluated or echoed into a command.
set -euo pipefail

# Issue numbers named with a closing keyword, from text on stdin, in order,
# without repeats. Same-repo references only (`#12`, not `owner/repo#12`).
closing_refs() {
  { grep -oiE '(^|[^[:alnum:]_/-])(close[sd]?|fix(e[sd])?|resolve[sd]?):?[[:space:]]*#[0-9]+' || true; } \
    | grep -oE '#[0-9]+$' | tr -d '#' | awk '!seen[$0]++'
}

# The release before TAG, from a tag list on stdin, by version order.
# Not `git describe`: release tags sit on main-side merge commits that
# develop never reaches (see next-version.sh). Prints nothing for the first.
prev_tag_from_list() {
  grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^v//' \
    | sort -t. -k1,1n -k2,2n -k3,3n \
    | awk -v t="${1#v}" '$0 == t { if (p != "") print "v" p; exit } { p = $0 }'
}

# PR numbers from merge-commit subjects on stdin.
pr_numbers() {
  sed -nE 's/^Merge pull request #([0-9]+) .*/\1/p'
}

# owner/repo: from Actions, else from the origin remote (no GraphQL needed).
repo_slug() {
  if [ -n "${GITHUB_REPOSITORY:-}" ]; then
    echo "$GITHUB_REPOSITORY"
  else
    git remote get-url origin | sed -E 's#^.*github\.com[:/]##; s#\.git$##'
  fi
}

main() {
  local tag="" dry=0 arg
  for arg in "$@"; do
    case "$arg" in
      --dry-run) dry=1 ;;
      -*) echo "Unknown option: $arg" >&2; exit 2 ;;
      *) tag="$arg" ;;
    esac
  done
  if [ -z "$tag" ]; then
    echo "Usage: $0 <tag> [--dry-run]" >&2
    exit 2
  fi

  local repo prev prs pr n info state is_pr issues=""
  repo=$(repo_slug)
  prev=$(git tag --list 'v*' | prev_tag_from_list "$tag")
  if [ -z "$prev" ]; then
    echo "$tag: no earlier release, nothing to close."
    return 0
  fi
  prs=$(git log --merges --format=%s "$prev..$tag" | pr_numbers)
  echo "$tag: PRs since $prev: ${prs:-none}" | tr '\n' ' '
  echo

  for pr in $prs; do
    for n in $(gh api "repos/$repo/pulls/$pr" --jq '.body // ""' | closing_refs); do
      issues="$issues"$'\n'"$n"
    done
  done
  issues=$(printf '%s\n' "$issues" | awk 'NF && !seen[$0]++')
  if [ -z "$issues" ]; then
    echo "$tag: no merged PR closes an issue."
    return 0
  fi

  for n in $issues; do
    info=$(gh api "repos/$repo/issues/$n" --jq '"\(.state) \(.pull_request != null)"')
    state=${info% *}
    is_pr=${info#* }
    if [ "$is_pr" = true ]; then
      echo "#$n is a pull request: skipped"
    elif [ "$state" != open ]; then
      echo "#$n is already $state: skipped"
    elif [ "$dry" -eq 1 ]; then
      echo "#$n: would comment \"Shipped in $tag.\" and close"
    else
      gh api -X POST "repos/$repo/issues/$n/comments" -f body="Shipped in $tag." >/dev/null
      gh api -X PATCH "repos/$repo/issues/$n" -f state=closed -f state_reason=completed >/dev/null
      echo "#$n: closed (Shipped in $tag.)"
    fi
  done
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
