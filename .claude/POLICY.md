# Ember Trove — Development Policy

The authoritative, full statement of how we build Ember Trove. `CLAUDE.md` is the
lean always-loaded summary; this file is the detail it links to. Read it before
starting non-trivial work. Rules here are normative ("MUST"/"NEVER").

---

## 1. Plan before you change

- For anything beyond a one-line fix, **plan first**. Explore the code, state the
  approach, and get sign-off before editing. Use plan mode for multi-file or
  architectural changes. A wrong plan caught early is cheaper than a wrong diff.
- Name the files you intend to touch and the patterns you intend to reuse *before*
  writing new code. If an existing helper does the job, use it — see §2.

## 2. Accuracy and reuse over speed

- Correctness, code reuse, and clarity beat velocity. There is no deadline that
  justifies a panic path or a copy-pasted helper.
- **Search before you write.** Grep for an existing function, newtype, or pattern
  first. The canonical patterns live in `.claude/patterns/`; the layer rules in
  `.claude/rules/`. Duplicating logic that already exists is a defect.
- Prefer `std` and crates already in `Cargo.toml` over new dependencies (§6).

## 3. Zero-panic, idiomatic Rust

- **NEVER** `.unwrap()`, `.expect()` (outside tests/build-time constants), or
  `panic!` in library/app code. Use `Result`/`Option` and `?`.
- **Enforced, not just reviewed.** `clippy::unwrap_used`, `expect_used`, and `panic`
  are denied in `[workspace.lints.clippy]` (root `Cargo.toml`); each crate opts in
  with `[lints] workspace = true`. Test code is exempt via `clippy.toml`
  (`allow-*-in-tests`). A genuinely-infallible call gets a *localised*
  `#[allow(clippy::unwrap_used)]` with a one-line justification — never a blanket
  relaxation. The gate (`cargo clippy … -D warnings`) now fails on a stray panic path.
- **UI context lookups** are a wiring invariant, not a runtime failure: use Leptos
  `expect_context::<T>()` (not `use_context().expect(...)`) — see `.claude/rules/leptos.md`.
- `thiserror` for library error enums; `anyhow` for application/binary glue.
- Prefer owned types at API boundaries; respect the borrow checker rather than
  reaching for `clone()` reflexively, but a clear `clone` beats a lifetime maze.

## 4. Test-driven development

- **Red → Green → Refactor.** Write a failing test first (`mod tests` or
  `api/src/tests.rs`-style integration), implement the minimum to pass, then refactor.
- New code lands **with** its tests in the same change. Bug fixes land with a
  regression test that fails before the fix.
- `ui/` (WASM/CSR) is exercised by integration/browser testing, not host unit tests;
  CI excludes it from `cargo test` (`--workspace --exclude ui`). Put testable logic in
  `common/` so it *can* be unit-tested.

## 5. Definition of done — all gates green

A change is **not done** until, locally and in CI, all of these are green:

```
cargo fmt --all --check                                   # formatting (enforced; see §11)
cargo clippy --workspace --exclude ui -- -D warnings      # api + common, warnings = errors
cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace --exclude ui
cargo check -p ui --target wasm32-unknown-unknown
```

`./scripts/verify.sh` runs the whole suite plus a clean-git-tree check. The git
**pre-commit** hook runs fmt+clippy; **pre-push** runs the tests. Install once with
`make hooks-install`. NEVER declare success on a red gate. NEVER `git commit --no-verify`
except for a genuine emergency, and say so in the commit body.

> rustfmt and clippy interact: applying `cargo fmt` can surface new clippy lints by
> rewriting closures/expressions. Always run clippy *after* fmt. See `.claude/ERRORS.md`.

## 6. Dependencies — research before adding

Before proposing a new crate:

1. Confirm nothing in `Cargo.toml` (workspace deps) or `std` already covers it.
2. Read the crate's **current** docs (docs.rs) and `CHANGELOG`; check it is actively
   maintained, its MSRV ≤ our toolchain, and its license is compatible (MIT/Apache-2.0).
3. Check `cargo audit` / RUSTSEC for open advisories.
4. Prefer the **latest stable** release; pin a major-compatible version in
   `[workspace.dependencies]` and let the leaf crates inherit with `.workspace = true`.
5. State *why* it's needed and what it replaces. New deps are reviewed, not assumed.

## 7. Git Flow & release hygiene

Branching model (also in `CONTRIBUTING.md`):

| Branch  | Pattern             | From      | Merges into        |
|---------|---------------------|-----------|--------------------|
| Feature | `feature/jc/<name>` | `develop` | `develop` (`--no-ff`) |
| Release | `release/<semver>`  | `develop` | `main` + `develop`, tag on `main` |
| Hotfix  | `hotfix/<name>`     | `main`    | `main` + `develop`, tag patch on `main` |

- **NEVER commit directly to `main` or `develop`.** Never force-push them.
- Conventional commits: `feat|fix|refactor|test|docs|chore|ci|build|style(scope): summary`.
- Use `./scripts/next-version.sh` to compute the next semver; `/release` for the flow.
- **A release is not shipped until every GitHub Actions workflow on the pushed ref is
  green** (`Release` *and* `CI`). After `git push origin main develop --tags`, poll
  `gh run list` until all runs `completed`; fix any `failure` at the root cause and ship a
  follow-up patch. A green Release beside a red CI still leaves `main` broken.

## 8. Documentation is part of the change

Every change updates, as applicable:

- **`CHANGELOG.md`** — under `## [Unreleased]`, Keep a Changelog categories
  (Added/Changed/Fixed/Security/Tooling/Documentation). Move to a versioned heading at release.
- **`README.md`** — new env vars, endpoints, commands, or setup steps.
- **Code comments** — explain *why*, not *what*; date non-obvious rationale (the
  `.cargo/audit.toml` style). Public items get `///` doc comments.
- **`.claude/`** — if you discover a pattern or a gotcha, record it (§10).

## 9. Be a partner, not a fan

- Push back on weak ideas, including the maintainer's. Disagree with reasons and a
  better alternative. Do **not** agree to be agreeable.
- Be technical and precise; assume an experienced reader. No filler, no false certainty.
  If you don't know, say so and find out. Report failures plainly with the output.

## 10. Learn every iteration

- When a non-obvious bug or fix recurs or costs real time, append it to
  `.claude/ERRORS.md` (symptom → cause → fix → date).
- When you establish a reusable code shape, add/refresh a file in `.claude/patterns/`
  and link it from the relevant `.claude/rules/*.md`.
- Keep `.claude/ROADMAP.md` current: state, backlog, and architecture decisions.
- Capture durable cross-session facts as Claude memory; keep `CLAUDE.md` lean (§11).

## 11. Tooling currency & formatting

- **Toolchain:** track the latest **stable** Rust. `rust-toolchain.toml` pins an exact
  version deliberately (not `stable`) so a new release can't silently introduce a
  clippy lint that breaks CI. Review each ~6-week stable; bump the pin in its own
  commit, run the full gate, and resolve new lints before merging.
- **Formatting:** default `rustfmt` is the law (no `rustfmt.toml`); the workspace is
  edition 2024, so editors must format with `--edition 2024`. CI and the pre-commit
  hook enforce `cargo fmt --all --check`.
- **CI must match the code:** workflows pin third-party Actions to commit SHAs (with a
  `# vX` comment); Dependabot proposes weekly bumps for `cargo` and `github-actions`.
  Keep `.github/workflows/*` in lockstep with toolchain, dependency, and Dockerfile changes.
- **Security advisories:** `.cargo/audit.toml` is the single source of truth for ignored
  RUSTSEC advisories; each ignore is transitive-only, dated, and reviewed every release.
  Never add inline `--ignore` flags in CI.

## 12. Coverage

- CI reports line coverage via `cargo llvm-cov` over `--workspace --exclude ui`.
  It is **report-only** today (no hard threshold) so it never blocks on the existing
  suite. New code should ship with tests that hold or raise coverage; convert the report
  to a `--fail-under` gate once the baseline is comfortable (one-line change in `ci.yml`).

---

See also: `CLAUDE.md` (summary + environment quirks), `.claude/rules/leptos.md`,
`.claude/rules/api.md`, `.claude/patterns/`, `.claude/ERRORS.md`, `.claude/ROADMAP.md`.
