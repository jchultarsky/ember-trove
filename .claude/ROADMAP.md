# Ember Trove — Roadmap & Architecture Decisions

Living document: current state, backlog, and the decisions behind the architecture.
Keep it current as part of each change (see `POLICY.md` §10).

## Current state (2026-07-19)

- **v2.24.1–v2.24.3 shipped (2026-07-18/19)** — the post-clustering polish arc,
  each prod-verified:
  - **v2.24.1**: Fit actually fits-to-content (`fit_transform` moved to
    `common::graph_layout`, host-tested), and ALL graph viewport math (Fit,
    auto-arrange framing, minimap click-to-centre + indicator rect) measures
    the `#graph-svg` canvas instead of the window — the sidebar biased fits
    ~230px right. Plus the retry-safe search-preset e2e (shared DB persists
    across Playwright retries; wipe before AND after — `.claude/rules/e2e.md`).
  - **v2.24.2**: daily TLS cert-expiry monitor (`cert-check.yml`, alerts <21
    days or bad chain; first scheduled run green 2026-07-19) and Rust
    toolchain 1.96 → 1.97.1 (zero new lints; watch transitive
    `proc-macro-error2` future-incompat via Leptos macros at the next Leptos
    bump). Release forced by a real constraint: **scheduled workflows only run
    from the default branch** — inert on develop.
  - **v2.24.3**: graph touch support. Root cause was deeper than missing drag
    wiring: canvas `touchstart` preventDefault suppressed ALL synthesized
    clicks, so nodes were fully inert on touch. Now: finger-drag persists
    (incl. iOS touchcancel), tap opens (replaces dblclick — double-tap is an
    OS zoom gesture), tap-pairs drive edge-create; one shared drag path for
    mouse + touch. Verified on prod at iPhone viewport with constructed
    TouchEvents; Julian confirms on-device.
  - Remaining graph non-goals: tag-dot taps fall through to node-open (5px
    dots aren't a touch target); hover cards stay mouse-only.
- **v2.24.0 shipped — graph auto-arrange re-architected:**
  the BFS-row `smart_layout` + load-time `force_layout_expanded` (both WASM-only,
  untested) are replaced by one pure engine, `common::graph_layout::cluster_layout`
  (force-directed; 10 host tests). Contract worth keeping: **gentle mode** (mostly
  seeded) uses Hooke springs with *seeded distances* as rest lengths and a local
  repulsion cutoff — it untangles without contracting the user's deliberate
  cluster separations; **pinned** seeds (initial page load pins all saved
  positions) never move, so loading the graph can't shift a hand-made layout;
  fresh layouts get a full FR anneal. Deterministic via UUID-hash jitter (WASM
  `Math.random` is gone from layout). The algorithm was tuned against Julian's
  real hand-made layout (backup: `~/projects/ember-trove-layout-backups/`).
  Both follow-ups from this pass are closed: the Fit button now
  fit-to-contents (PR #52, which also fixed the sidebar-width bias in all
  viewport math incl. the minimap indicator), and the "14 stale
  `node_positions` rows" were a diagnostic artifact — they are archived
  nodes, invisible to `/api/nodes` without `include_archived=true`
  (`list_positions` JOINs `nodes`, so true orphans cannot appear; the FK
  cascade is fine). Verified against prod 2026-07-18: 84 positions = 70
  active + 14 archived, zero orphans.
- **v2.23.0 shipped** — the "trust the suite" release (2026-07-17 review plan,
  below). Coverage inverted-vs-risk is corrected: registration + behavior
  tests for the six previously-untested privileged route groups (admin,
  backup, metrics, export, attachments, editor-prefs — 91→110 api tests);
  e2e specs for the graph view (`graph.spec.ts` — the largest UI surface, was
  untested); repo-layer tests against real Postgres (`pg-tests` feature +
  `#[sqlx::test]`, new CI job `repo tests (postgres)`); coverage floor raised
  17→24 (measured baseline 25.96%). Both product decisions resolved:
  **webhooks** shipped a UI (`/webhooks`) — and building it surfaced/fixed a
  secret-wiping `PUT` bug (unconditional secret write vs. masked-secret reads;
  now `deser_double_opt` PATCH semantics). **`/search`** kept (sidebar box
  already navigates there); closed the real gap with `Go to Search`/`Go to
  Webhooks` palette commands. Also folded in the earlier unreleased work:
  three security fixes (rate-limit `/share/{token}`, node-scoped token revoke,
  webhook-dispatch DNS re-vet + pinning — `api/src/ssrf.rs`) and the
  community-health set (SECURITY.md, CoC, issue/PR templates, `license = MIT`).
- **v3 groundwork (post-2.23.0, on `feature/jc/local-auth`):** zero-AWS local
  login via a bundled Keycloak issuer (`./scripts/dev-local.sh`) whose
  `cognito:groups` protocol mapper leaves the token path unchanged; only API
  change is a Cognito-only guard on `/auth/change-password`. Verified
  end-to-end (login → `/api/auth/me` roles:["admin"]). Known follow-up: the
  Keycloak login page renders unstyled through the proxy (cosmetic).
- **v2.22.3 shipped** — first release under the personal `jchultarsky` account
  (repo transferred from `jchultarsky101`, 2026-07). Patched RUSTSEC-2026-0193
  (ammonia mXSS — the user-markdown sanitizer, a stored-XSS vector here) and
  RUSTSEC-2026-0185 (quinn-proto; verified an unreachable orphan lock subtree).
  Owner-pinned references repointed (GHCR image paths in deploy/, badges,
  docs); GHCR packages pre-seeded, made public, and repo-linked with Actions
  Write under the new owner; prod deploy SSH key rotated (dedicated GHA key
  lives only in `LIGHTSAIL_SSH_KEY`; personal key `~/.ssh/lightsail-julian`,
  `ssh trove`) and proven end-to-end by the release deploy.
- **Repo is public BY INTENT** — declared an open-source project (sole
  contributor today). Community set added: SECURITY.md (private vulnerability
  reporting enabled on the repo), Contributor Covenant 2.1, issue/PR
  templates, `license = "MIT"` in all crate manifests.
- **2026-07-17 full-codebase review** (backend + frontend + test-infra survey)
  produced the plan of record below. Three concrete security findings are in
  progress on `feature/jc/security-hardening` (target v2.22.4).

## Plan of record (2026-07-17 review)

- **v2.24.0 — "keyboard & a11y" (planned 2026-07-18):** unify the ad-hoc
  keyboard handling into one model. Current state (inventory): no central
  dispatcher — 3 window-level keydown listeners (layout, my_day, inbox_triage)
  + ~15 element-scoped handlers, the editable-guard copy-pasted 3× with the
  triage copy diverging (omits BUTTON/contenteditable), the `help.rs` shortcut
  table display-only and free to drift from real dispatch, a leaked Cmd-K
  listener (`layout.rs:131`, no `on_cleanup`), and the graph with zero keyboard
  support. Target: one global dispatcher owned by `Layout`; a shortcut registry
  whose match logic is a pure `common/` fn (host-testable) that also generates
  the help table (no drift); a `KeyboardScope` context replacing "which
  component is mounted" (Global/MyDay/Graph + exclusive Triage/Palette/Modal);
  one shared `is_editable_target()`. Phases, each a shippable PR:
  0. Extract `is_editable_target()` + reconcile the 3 copies; fix the Cmd-K
     leak. Two bug fixes, no UX change. *(in progress)*
  1. Registry + pure match fn in `common/`; help table generated from it;
     collapse the two `layout.rs` listeners into one.
  2. Overlay-scope suppression: `in_overlay` flag per registry row +
     `overlay_active` arg to `match_global`, so navigating keys don't leak
     through the palette/help (fixed a real bug — `g` navigated through the
     open help modal). *Reframed from the original "full KeyboardScope + view
     registration": that was mostly redundant with the phase-0 editable-guard,
     and the view-scope abstraction's real consumer is phase 3 — so it folds
     there rather than shipping as unused infrastructure.*
  3. Graph keyboard/a11y baseline (shipped): each node is a focusable
     `button` (tabindex/role/aria-label) with a focus ring and Enter/Space
     activation — the graph is Tab-navigable and screen-reader-legible, done
     with **native focus** (no custom cursor). *Finding: this is the THIRD
     phase where the `KeyboardScope` model proved unnecessary (0 used the
     editable-guard, 1 the registry, 2 overlay flags, 3 native focus). The
     scope model was a solution without a problem — **dropped from the plan.**
     The only thing that would want it is optional arrow-key SPATIAL graph
     navigation (a "3b" UX enhancement, not an a11y requirement); revisit only
     if that's wanted.*
  4. a11y sweep on touched surfaces (`aria-selected`/`aria-activedescendant`).
  Boundary: do NOT centralize the palette/triage internal state machines —
  centralize guard/registry/dispatch/scope only. Risk to watch: a panic in the
  dispatcher poisons ALL keyboard handling (the v2.21.1 lesson), so its core
  is a pure fn and every e2e collects `pageerror`. Only destructive shortcut is
  `d`=delete, and both paths are reversible (soft-delete/undo) — no new
  security surface.

- **v2.22.4 — security patch (in progress):**
  1. `/share/{token}` joins the rate-limited router group — it was the only
     unauthenticated, ungoverned endpoint, and it performs a token lookup.
  2. `revoke_share_token` scopes the DELETE to the node in the path
     (`WHERE id = $1 AND node_id = $2`) — previously any node owner could
     revoke any share token by id (cross-node).
  3. Webhook dispatch re-resolves and re-vets the target host, then pins the
     HTTP client to the vetted addresses (`resolve_to_addrs`) — closes the
     DNS-rebinding TOCTOU left by create/update-time-only SSRF validation.
  Plus: clear the Dependabot backlog (incl. the month-old tower-http 0.6→0.7
  semver-major, which needs a real review).
- **v2.23.0 — "trust the suite":** the review found coverage inverted vs risk.
  Registration + behavior tests for the six untested route groups (admin,
  backup, metrics, export, attachments, editor_prefs — the privileged
  surfaces); e2e specs for the knowledge-graph half (graph_view.rs 2.4k lines,
  node_editor, node_view have none today); repo-layer tests against real
  Postgres (reuse the CI migration-validation container); raise the coverage
  floor above 17% as this lands. Product decisions due: **webhooks** —
  DECIDED 2026-07-17: shipped the UI (`/webhooks`; building it surfaced and
  fixed the secret-wiping update semantics). **`/search` view** — DECIDED
  2026-07-17: KEEP. The "orphaned" claim was overstated (the sidebar search
  box navigates there on Enter / "View all"); the real gap was palette
  parity, closed with `Go to Search` (+ `Go to Webhooks`) commands. Do not
  fold the full search page into the palette — presets/filters/full results
  are a different job than quick-jump.
- **v3 candidates — OSS launch:** self-contained local auth (Keycloak/dex with
  a `cognito:groups` claim mapper) to restore zero-AWS clone-and-run —
  **promoted from deferred**: it is the main adoption barrier now the repo is
  public by intent. A11y systematization beyond modals (~44 aria occurrences
  crate-wide; keyboard dispatch hand-rolled in 23 files).
- **Opportunistic refactors** (do while touching the area, never big-bang):
  consolidate the three parallel task-row components (task_row / task_panel /
  inbox_view); extract a shared debounce helper (pattern re-derived in 6
  files); merge the three `repo/search.rs` query builders (kills the
  `too_many_arguments` allows); adopt `#[from] sqlx::Error` in repos (~146
  `Internal(format!)` sites); split `graph_view.rs` (move pure layout
  algorithms out) and `routes/nodes.rs` (27 handlers); route node export
  through the UI API client (raw `<a href>` today); drop the duplicate
  `nodes(owner_id)` index (migration 021 duplicates 001).

## Prior state (2026-06-10)

- **Released & prod-verified:** v2.22.0 — the ROADMAP backlog cleared. All
  new surfaces hand-tested live after deploy: calendar day-click captured a
  due-today task; the carryover prompt's Yes re-stamped and cleared the
  badge; the Overdue section rendered, counted, and folded. One operational
  observation, diagnosed and **fixed in v2.22.1**: deploys forced open tabs
  to re-login because `AuthGate` treated any `/api/auth/me` failure as
  Unauthenticated — including the seconds of API downtime during the
  container restart. The probe now retries transient failures (network/5xx)
  with ~23 s of backoff; only an authoritative 401/403 ends the session.
  **Live-verified on the v2.22.2 deploy** (2026-06-10): the tab was reloaded
  inside the restart window (health watcher caught API down) and came back
  authenticated on the new bundle — no login bounce. The pre-fix behavior at
  that exact moment was a forced Cognito re-login. My Day carryovers now
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
- **Coverage is now a floor gate, not report-only.** `cargo llvm-cov … --fail-under-lines 24`
  (baseline 25.96% on 2026-07-17, post-"trust the suite"; previously 17 under an 18.7%
  baseline, 2026-06-05). The floor sits ~2 points under the baseline so it never blocks the
  existing suite but catches a regression; raise deliberately as the suite grows. (2026-06-05,
  raised 2026-07-17)
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
