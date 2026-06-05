#!/usr/bin/env bash
# Full verification suite for Ember Trove.
# Run before committing, releasing, or after major refactors.
# Usage: ./scripts/verify.sh

set -e
export PATH="$HOME/.cargo/bin:$PATH"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

PASS=0
FAIL=0
ERRORS=""

run_step() {
  local label="$1"
  local cmd="$2"
  echo -n "  [$label] ... "
  if output=$(eval "$cmd" 2>&1); then
    echo "PASS"
    ((PASS++)) || true
  else
    echo "FAIL"
    ((FAIL++)) || true
    ERRORS="$ERRORS\n--- $label ---\n$(echo "$output" | head -20)\n"
  fi
}

echo ""
echo "=== Ember Trove Verification Suite ==="
echo ""

run_step "cargo fmt --check"      "cargo fmt --all --check"
# Be explicit about `--workspace --exclude ui` (matching CI and the git hooks)
# rather than leaning on `default-members`; if `ui` is ever added to
# `default-members`, the bare forms would silently try to host-compile WASM.
run_step "cargo check"            "cargo check --workspace --exclude ui --quiet"
# NOTE: --quiet must precede `--`; anything after `--` is forwarded to rustc,
# which has no --quiet flag (this previously failed every clippy step).
run_step "cargo clippy"           "cargo clippy --workspace --exclude ui --quiet -- -D warnings"
run_step "cargo check (wasm32)"   "cargo check -p ui --target wasm32-unknown-unknown --quiet"
run_step "cargo clippy (wasm32)"  "cargo clippy --quiet -p ui --target wasm32-unknown-unknown -- -D warnings"
run_step "cargo test"             "cargo test --workspace --exclude ui --quiet"
run_step "git status clean"       "[ -z \"\$(git status --porcelain)\" ]"

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [ "$FAIL" -gt 0 ]; then
  echo ""
  echo "Failures:"
  echo -e "$ERRORS"
  exit 1
fi

echo "All checks passed. Ready to commit/release."
exit 0
