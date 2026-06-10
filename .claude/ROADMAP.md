# Ember Trove — Roadmap & Architecture Decisions

Living document: current state, backlog, and the decisions behind the architecture.
Keep it current as part of each change (see `POLICY.md` §10).

## Current state (2026-06-10)

- **Released & prod-verified:** v2.22.0 — the ROADMAP backlog cleared. All
  new surfaces hand-tested live after deploy: calendar day-click captured a
  due-today task; the carryover prompt's Yes re-stamped and cleared the
  badge; the Overdue section rendered, counted, and folded. One operational
  observation, diagnosed: each deploy forces open tabs to re-login. NOT
  key rotation — the deploy reuses the host's persistent `deploy/.env.prod`
  (stable COOKIE_KEY). Root cause is client-side: `AuthGate` treats ANY
  `/api/auth/me` failure as Unauthenticated, so the seconds of API downtime
  during the container restart bounce the tab to Cognito (new PKCE state →
  login form) even though the session cookie is still valid. Candidate fix:
  distinguish 401 from network/5xx in AuthGate and retry with short backoff
  before redirecting. My Day carryovers now
  prompt "still today?" (Yes re-stamps, No drops to backlog) and overdue
  tasks group into a foldable red-accented section (binary `focus_date` ADR
  unchanged); the Calendar adds click-a-day quick capture (`data-date` cells,
  inline composer → standalone task due that day); focus traps completed on
  the last two modals (create-node, add-favorite); the saved-search presets
  UI turned out to already exist (stale backlog claim) and is now pinned by
  e2e. Suite: 19 Playwright specs + host unit tests for every new pure
  function. Prior same-day releases: v2.21.4 (palette ranking: commands beat
  body-text node matches), v2.21.3 (triage/palette e2e), v2.21.2 (e2e
  foundation), v2.21.1 (WASM hotfixes), v2.21.0 (usability review).
- **Prior (v2.21.3):** — e2e suite grown to 13 specs: triage flows (`t`/`s`/`a`
  decisions with API-verified server state, skip-wrap, no-changes exit) and the
  command palette (synonym matching, navigation dispatch, dark-mode round-trip,
  node search, context commands). Only app change: a `data-testid` on the
  triage card. The first cloud run caught a real spec bug (Cmd+K fired before
  the WASM listener registered on cold runners — invisible on warm local
  stacks); fixed with a render gate and recorded in `.claude/rules/e2e.md`,
  which now carries five selector/timing lessons. Verified on prod
  (`/api/health` → 2.21.3).
- **Prior (v2.21.2):** — Playwright e2e smoke suite (`e2e/`, `scripts/e2e.sh`,
  CI job `e2e`), the direct answer to the v2.21.1 lesson that host-side gates
  cannot see WASM runtime bugs. Five specs (shell, NL quick capture,
  delete→undo→restore, zombie-listener regression, editor autosave) run
  against a dedicated Docker stack: api built with the new `e2e-bypass` cargo
  feature (synthetic non-admin user; release images build featureless so the
  code path never ships, and runtime arming needs `E2E_AUTH_BYPASS=1`),
  tmpfs Postgres, separate compose project. Playwright runs in its official
  Docker image — no local Node. Every push now gets browser-level coverage;
  release verified on prod (`/api/health` → 2.21.2). Grow specs alongside new
  UI surfaces.
- **Prior (v2.21.1):** — hotfix for two UI bugs found by live prod testing of
  v2.21.0 minutes after release: (1) `MyDayView` leaked its window keydown
  listener on unmount (the handle's Drop does not detach; a zombie listener
  panicked on disposed signals and poisoned all WASM event dispatch);
  (2) toasts pushed after an `.await` in `wasm_bindgen_futures::spawn_local`
  were silently dropped (`use_context` has no owner there) — undo toasts never
  rendered, nor had several older continuation toasts. Both lessons recorded
  in `.claude/ERRORS.md` and `.claude/rules/leptos.md`. Fixes verified live in
  prod post-deploy: the v2.21.0 crash repro (My Day → tab switch → keypress) is
  clean, and the delete → Undo → restore cycle works end-to-end. **Process lesson:**
  post-release live testing in prod caught in 10 minutes what unit tests and
  clippy structurally cannot — WASM runtime behavior needs the browser; the
  e2e-harness backlog item just got its strongest argument yet.
- **Prior (v2.21.0):** — the full 2026-06-09 UI usability review, shipped across ten
  feature branches and verified on prod (`/api/health` → 2.21.0, DB ok).
  **Trust tier:** editor autosave + create-mode localStorage drafts + save-state
  indicator (with server-side version-snapshot dedupe and 15-min "Edited" activity
  coalescing); optimistic-rollback sweep (all 18 fire-and-forget mutations now revert
  + toast on failure); undo-toast deletion via soft delete (migration 030 `deleted_at`
  tombstones on tasks/notes, `POST /{tasks,notes}/:id/restore`, 30-day purge at startup
  + daily). **Feature tier:** unlinked mentions with one-click wikilink conversion
  (`common::markdown::link_first_mention`); NL quick-add tokens (`common::quickadd`,
  "buy milk friday p1"); keyboard inbox triage ("Process" mode, t/s/a/d/j/k); command
  palette app commands with shortcut hints + node-context actions; a11y pass (modal
  focus traps + focus return, route-change `document.title` + focus, live-region
  toasts, ARIA tabs, labeled priority dots); local graph panel on node pages +
  orphans-only lens on the global graph; skeletons for Search/Templates;
  `prefers-color-scheme` default. Also fixed: a failed node load can no longer be
  saved back as an empty body. Local Docker stack verified pre-release (migration 030
  applied cleanly on `postgres:16`).
- **Prior (v2.20.x):** login restoration patches (CSP 303 redirect, `jsonwebtoken`
  `aws_lc_rs` backend), auth rate-limit tuning, pre-commit secret scan, fixed local
  Docker stack (`COOKIE_KEY` from `.env.local`).
- **Prior (v2.19.x):** closed the deep security review/audit (sprints 7–9): CSP nonce +
  `strict-dynamic` (removed `script-src 'unsafe-inline'`), Cognito admin hardening,
  activity-log scoping, backup/restore authz, rate-limiting coverage, full
  sqlx-parameterization sweep.
- **Toolchain:** Rust pinned to `1.96` (current stable) in `rust-toolchain.toml`;
  workspace edition 2024, resolver 2.
- **Pipeline:** `CI` (check/clippy/fmt/audit/migrations/coverage/docker-build) +
  `Release` (GHCR images, GitHub Release, EC2 deploy) on tag push.

## Backlog / candidate work

- 2026-06-09 usability review: **fully shipped** across v2.21.0–v2.22.0
  (see Current state), including every follow-on nice-to-have and the palette
  ranking fix it surfaced. Only deliberate deferral kept: block references —
  heading links (`[[Note#Heading]]`) cover most of the value; revisit only if
  transclusion demand materializes.
- ~~UI test strategy~~ **Decided 2026-06-10: Playwright** (`e2e/`, CI job
  `e2e`) after v2.21.1 proved host-side gates can't see WASM runtime bugs.
  Smoke-level today (5 specs); grow specs alongside new UI surfaces, and keep
  pushing pure logic into `common/` for unit coverage.
- Optional: `lldb-dap` for editor step-debugging (not installed; editor-local tooling,
  not a repo deliverable).
- **Self-contained local auth (deferred):** local login needs a Cognito pool — there's no
  bundled IdP since the Keycloak→Cognito migration. README now documents "bring your own
  Cognito". A local OIDC container (Keycloak/dex with a `cognito:groups` claim mapper) would
  restore zero-AWS clone-and-run, but partially reverses that migration for local — revisit
  only if the cloner experience needs it.

## Architecture decisions (ADR-lite)

- **Edition 2024 + resolver 2.** Latest stable edition; matches toolchain currency policy.
- **Modern AWS TLS stack.** `default-features = false` + `default-https-client`
  (rustls 0.23 + aws-lc-rs) on AWS SDK crates — eliminated the legacy rustls 0.21 advisory
  chain. Do not reintroduce the `rustls` feature. (2026-05-29)
- **`audit.toml` as single source of truth** for ignored RUSTSEC advisories; transitive-only,
  dated, reviewed per release.
- **Git Flow** (feature/release/hotfix). Heavyweight for a solo maintainer — even its author
  concedes trunk-based fits continuously-deployed web apps better — but the release/CD tooling
  (`next-version.sh`, tag-triggered `Release`) is built around it, so it stays. Reassess if/when
  contributor count or release cadence changes.
- **Default rustfmt, no `rustfmt.toml`.** Adopted 2026-06-05 with a one-time workspace
  reformat; enforced by hook + CI. Editors format with `--edition 2024`.
- **SHA-pinned GitHub Actions + Dependabot.** Supply-chain hardening consistent with the
  project's security posture; Dependabot keeps pins current.
- **Coverage is now a floor gate, not report-only.** `cargo llvm-cov … --fail-under-lines 17`
  (baseline ~18.7% on 2026-06-05). The floor sits under the baseline so it never blocks the
  existing suite but catches a regression; raise deliberately as the suite grows. (2026-06-05)
- **`cargo-deny` added for licenses + bans + sources only** (2026-06-05). Advisories stay with
  `cargo audit` (`.cargo/audit.toml` is the single source of truth) so the two never diverge —
  cargo-deny runs only the non-overlapping checks, resolving the earlier "avoid overlap"
  deferral. Workspace crates are `publish = false` and skipped via `[licenses].private`; three
  permissive transitive licenses (BSL-1.0, CDLA-Permissive-2.0, bzip2-1.0.6) are allow-listed
  with provenance comments in `deny.toml`.
- **`focus_date` is a binary UI model (`today | None`).** The wire type stays
  `Option<NaiveDate>` and the API accepts any date, but the My Day Kanban only ever writes
  `Some(today)` or clears it — there is **no future-date picker on the daily surface**, by
  deliberate user choice ("today or not today", v2.6.0). `due_date` is the editable deadline
  and lives in the task-editor modal. `is_in_my_day`/`list_my_day` still handle carryovers
  (past `focus_date`, not done). A genuine "schedule for a future day" need should land next
  to `due_date`, never as a Kanban quick action — keep the daily surface lean. (2026-04-28)
- **Quick-capture target is a `Task` with `node_id IS NULL`, not a Node.** `/api/inbox/quick`
  (+ the iOS Web Share Target) creates a triage Task surfaced by `/tasks/inbox`
  (`tasks WHERE node_id IS NULL`); wire format `{title?, body?}`, coalesced + truncated to 500
  chars server-side. A 6th `NodeType::Inbox` variant was considered and **rejected** — it
  would have meant a migration plus duplicate sidebar/filter/dashboard wiring for no
  behavioural win, and Notes need a parent node so couldn't be the inbox surface. Future
  capture paths (command palette, Apple Shortcut, third-party clippers) MUST hit
  `/api/inbox/quick` — do not invent a parallel create-node path. (2026-04-27)
