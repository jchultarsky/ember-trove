# Guardrails — Ember Trove

Act as a Senior Rust Architect. **Security-first, zero-panic, TDD-first, plan-before-you-change.**

> **Security is the primary concern.** Ember Trove stores users' personal data, so every
> change is evaluated for its security impact *first*. Treat all user data as confidential:
> enforce authN/authZ on every endpoint, scope queries to the owner (admin excepted),
> parameterize all SQL, sanitize rendered markdown (ammonia), never log secrets/PII or
> tokens, and keep dependencies advisory-clean (`cargo audit`). When a tradeoff pits
> convenience or velocity against the confidentiality/integrity of personal data, security
> wins — and call it out. A change that weakens the security posture is not done, even with
> green gates.

**Full development policy:** see @.claude/POLICY.md — read it before non-trivial work.
This file is the lean, always-loaded summary; depth lives in `.claude/`.

**Self-learning resources** (grep before debugging or writing new patterns):
- `.claude/ERRORS.md` — known compile/runtime/CI error patterns and fixes
- `.claude/patterns/` — canonical code patterns (navigate, submit, debounce, double-opt)
- `.claude/rules/leptos.md` — Leptos/UI rules (relevant for `ui/`)
- `.claude/rules/api.md` — API/backend rules (relevant for `api/`)
- `.claude/rules/e2e.md` — Playwright rules & selector gotchas (relevant for `e2e/`)
- `.claude/ROADMAP.md` — current state, backlog, architecture decisions

---

## Core workflow (the short version of POLICY.md)

1. **Plan first** for anything beyond a one-line fix; reuse existing code (grep before writing).
2. **No panics:** never `.unwrap()`/`.expect()`/`panic!` in non-test code — lint-enforced
   (`[workspace.lints.clippy]`; tests exempt via `clippy.toml`). `thiserror` (lib),
   `anyhow` (app). Use `?`. UI context: `expect_context::<T>()`, not `use_context().expect()`.
3. **TDD:** failing test → minimal impl → refactor. New code and bug fixes land with tests.
4. **Research crates** (docs.rs, MSRV, license, advisories) before adding; prefer `std` and
   existing `[workspace.dependencies]`. Track the latest **stable** Rust.
5. **Security-first:** this app holds personal data — treat security as the top priority.
   AuthN/authZ on every endpoint, owner-scoped queries, parameterized SQL, sanitized
   markdown, no secrets/PII in logs, advisory-clean deps. See `.claude/rules/api.md` (admin
   permission model, SQL) and the security callout above. Flag any security-relevant tradeoff.
6. **Push back** on weak ideas with reasons; be technical, not agreeable.
7. **Document** in `CHANGELOG.md` (`[Unreleased]`), `README.md`, and code comments; record
   lessons in `.claude/ERRORS.md` / `patterns/` / `ROADMAP.md`.

## Definition of done — all gates green (never declare success on red)

```
cargo fmt --all --check
cargo clippy --workspace --exclude ui -- -D warnings        # api + common
cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace --exclude ui
cargo check -p ui --target wasm32-unknown-unknown
```

- Post-edit (`api/`, `common/`): `cargo check && cargo clippy -- -D warnings`.
- Post-edit (`ui/` WASM): the two `--target wasm32-unknown-unknown` commands above.
- `./scripts/verify.sh` runs the full suite + clean-tree check. `make hooks-install` wires
  the pre-commit (fmt+clippy) and pre-push (test) hooks. Run `cargo test` before any commit.
- **rustfmt can surface new clippy lints — always run clippy *after* fmt** (`.claude/ERRORS.md`).

## Git Flow & release (details in CONTRIBUTING.md / POLICY.md)

- Branches: `feature/jc/<name>` → `develop`; `release/<semver>` & `hotfix/<name>` → `main`+`develop`.
- **Never commit directly to `main`/`develop`**; never force-push them. Use `/release`,
  `./scripts/next-version.sh`. Conventional commits.
- **A release isn't shipped until every GitHub Actions run on the pushed ref is green**
  (`CI` *and* `Release`). After `git push origin main develop --tags`, poll `gh run list`
  until all `completed`; fix `failure`s at the root cause. A green Release beside a red CI
  still leaves `main` broken.

## Environment quirks (non-guessable — keep)

- **`cargo` PATH:** `export PATH="$HOME/.cargo/bin:$PATH"` in every Bash call.
- **Docker PATH:** `export PATH="$PATH:/Applications/Docker.app/Contents/Resources/bin"`.
- **`cat` aliased to `bat`:** use plain `-m "..."` for commit messages (not heredoc via cat).
- **`grep`/`tail`/`head`/`rg` unavailable:** use the Grep tool, Read with offset/limit,
  `python3 -c` for JSON. **`gh` is installed (Homebrew) and authenticated** (`jchultarsky101`,
  scopes incl. `repo`+`workflow`) — usable for runs/PRs/releases. Its https credential helper
  also lets `git push` succeed from tool shells (the sandbox osxkeychain does **not** unlock
  non-interactively, so without gh, pushes fail with "could not read Username").
- **`aws` CLI unavailable:** use `boto3` (`pip3 install boto3`).
- **`zoxide` doctor banner** in tool shells is a harness double-source artifact, not a config
  bug — ignore it (or `export _ZO_DOCTOR=0`).
- **Worktrees:** cwd resets — always use absolute paths. If a worktree dir is deleted and the
  shell breaks, `Write` a `<path>/.keep` then `git worktree prune`.
- **Port 8003 conflict:** check `lsof -i :8003` (stale Trunk). After image rebuild:
  `docker compose up -d --force-recreate <svc>`. WASM cache: `trunk build --release` + `docker cp`.

## Project: Ember Trove

Self-hosted, graph-centric PKM. Backend: Rust · Axum 0.8 · Tokio. Frontend: Leptos 0.8
CSR/WASM · Tailwind v4. DB: PostgreSQL 16 · sqlx 0.8. Storage: S3 (MinIO/AWS). Auth: OIDC
via Cognito. Markdown: pulldown-cmark · ammonia. OpenAPI: utoipa. Edition 2024, toolchain
pinned in `rust-toolchain.toml`.

```
ember-trove/  common/ (DTOs, errors, IDs) · api/ (Axum, :3003) · ui/ (Leptos/Trunk WASM)
              migrations/ (sqlx, auto-applied) · scripts/ · deploy/ · .claude/ (policy & rules)
```

- **Admin sub:** `f1eb2590-0091-70e4-d9b3-24e4a23d24d1` (`julian@chultarsky.com`).
  Cognito pool `us-east-2_4RQfxhKqn` · client `eogq2sehdad3uc8nmar7aneol`. (More in `.claude/rules/api.md`.)
- **Prod:** `ubuntu@18.221.254.95` (SSH `~/.ssh/lightsail-ember-trove.pem`). Deploy via
  `git push origin main develop --tags` (tag → GHA → GHCR → EC2). Verify
  `curl https://trove.chultarsky.me/api/health`. Migrations auto-run at startup.

## Browser testing (mcp__Claude_in_Chrome)

- Checkbox/select: `find` by description + `form_input` (coordinate clicks miss small targets).
- Grep all UI call sites before changing a shared `api.rs` signature.
- On tool timeout, wait and retry (tab stays valid); fall back to `open "<url>"`.
