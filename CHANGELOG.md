# Changelog

All notable changes to Ember Trove are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed — My Day task titles wrap on phone widths
On an iPhone the My Day column is narrow and the row's `truncate` cut every
long title to one ellipsised line. Small screens now wrap the full title
across multiple lines (priority dot and due date stay pinned to the first
line); `sm:` and up keep the single-line truncation so desktop list density
is unchanged. The Inbox and node task-panel rows already wrapped — only the
`KanbanTaskRow` truncated. E2e pins both behaviors by measuring the rendered
title height at 375px vs 1280px.

## [2.24.3] - 2026-07-19

### Fixed — the graph works on touch devices
On a phone or tablet the graph was pan/zoom only — and worse, the canvas
touchstart's preventDefault suppressed the browser's synthesized clicks, so
nodes couldn't be dragged, opened, or used for edge-create at all. Nodes now
handle touch directly: drag with a finger (position persists on release, and
on iOS touch-cancel), tap to open the node (double-tap is an OS zoom gesture,
so tap replaces double-click on touch), and tap to pick source/target in
edge-create mode. Mouse and touch drags share one code path.

## [2.24.2] - 2026-07-18

### Tooling — Rust toolchain 1.96 → 1.97.1
Routine ~6-week stable review (policy §12). Full gate green with zero new
clippy lints. New watch item surfaced by 1.97: a future-incompatibility
warning for the transitive `proc-macro-error2 2.0.1` (via the Leptos macro
crates) — build-time only, upstream's to fix, re-check on the next Leptos bump.

### Tooling — daily TLS cert-expiry monitor for prod
The 2026-06-17 HTTPS outage was a silent renewal failure: nothing alerted
until the cert actually expired. A scheduled GitHub Actions workflow
(`cert-check.yml`) now checks the live cert daily and fails — emailing the
maintainer via normal workflow-failure notifications — when under 21 days
remain (healthy renewals keep it ≥ ~60) or when the served chain doesn't
verify. Lives in the repo rather than on the host precisely because host-side
renewal state is where the config drifted unseen last time.

## [2.24.1] - 2026-07-18

### Fixed — Fit actually fits (and the minimap stops lying about the viewport)
The graph's Fit button hard-reset to 100% zoom at the origin, which could
leave a clustered layout entirely off-screen. It now computes a real
fit-to-content transform (`common::graph_layout::fit_transform`, moved from
the UI so it is host-tested) from the current positions. All viewport math —
Fit, auto-arrange framing, minimap click-to-centre, and the minimap's
viewport-indicator rectangle — now measures the actual graph canvas
(`#graph-svg`) instead of the window, removing a sidebar-width bias that
overshot fits to the right and oversized the minimap indicator. E2e
regression: far-away nodes are brought into the viewport by Fit.

## [2.24.0] - 2026-07-18

### Changed — auto-arrange clusters connected nodes instead of laying rows
The graph's Auto-arrange button ran a BFS-layered hierarchical layout: every
node was assigned a layer and each layer became a horizontal row — connectivity
had almost no influence on proximity. It now runs a force-directed cluster
layout (`common::graph_layout::cluster_layout`, pure + host-tested): edges
attract, nodes repel, so hubs become star centres with satellite rings that
widen with degree, and weakly-connected groups separate into distinct clusters.
The refinement is **mental-map preserving** — it seeds from the current
arrangement, keeps the user's chosen edge lengths as spring rest lengths
(deliberate long bridges between clusters are not contracted), and preserves
the coordinate frame. Initial page load uses the same engine with all saved
positions *pinned*: a node that has never been placed settles near its
neighbours instead of being scattered randomly, and saved positions never
move. Deterministic (UUID-hash jitter, no `Math.random`), with a minimum
node-spacing pass and grid packing for never-placed disconnected components.
Replaces ~450 lines of WASM-only untested layout code; covered by 10 unit
tests plus an e2e regression ("clusters a star around its hub, not in rows").

### Added — the graph is keyboard-navigable and screen-reader-legible (keyboard phase 3)
Graph nodes were mouse-only and invisible to assistive tech. Each node is now
a focusable `button` — `tabindex="0"`, `role="button"`, an `aria-label`
("〈title〉, 〈type〉 node") — with a sky-blue focus ring and Enter/Space
activation (open the node, or select it in edge-create mode). Tab moves between
nodes; done with native focus, so no custom cursor. (Arrow-key *spatial*
navigation and the once-planned `KeyboardScope` model are not needed for this
and are dropped/deferred — see `.claude/ROADMAP.md`.) Covered by a new
`graph.spec.ts` case.

### Fixed — global shortcuts no longer leak through an open overlay (keyboard phase 2)
With the help modal open, pressing a global shortcut like `g` navigated away
*through* the modal — help (unlike the other modals) doesn't move focus into
itself, so the editable-guard didn't catch it. Each registry entry now carries
an `in_overlay` flag and `match_global` takes an `overlay_active` argument: the
navigating shortcuts (`n`/`g`/`/` and the contextual `d`) are suppressed while
the palette or help owns the keyboard, but the overlay-control keys
(`⌘K`/`?`/`Escape`) still work. (Regression-tested red→green in
`palette.spec.ts`.) This is the overlay-scope slice of the phase-2 plan; the
broader view-scope model folds into phase 3, where the graph keyboard cursor
gives it a concrete consumer — see `.claude/ROADMAP.md`.

### Changed — shortcut registry (unified-keyboard-model, phase 1)
The six global shortcuts (`n` `g` `/` `⌘K` `?` `Escape`) now live in one
registry, `common::keyboard::GLOBAL`, that drives **both** dispatch (via the
host-tested pure `match_global`) **and** the help modal's "Anywhere" table
(rendered from the same table) — so a documented shortcut that doesn't fire,
or vice-versa, is now impossible. The two `layout.rs` window listeners are
collapsed into one owned dispatcher with a single `on_cleanup`. No user-facing
behavior change. Adds an e2e for the help modal (`?`), which was untested.
(View-specific shortcuts and the contextual `d` migrate to the registry with
the phase-2 scope model.)

### Fixed — keyboard-handling foundation (unified-keyboard-model, phase 0)
First step of the v2.24.0 keyboard/a11y plan (`.claude/ROADMAP.md`), fixing two
real bugs with no UX change:
- The "is the focused element editable?" guard that stops single-key shortcuts
  from firing while typing was copy-pasted in three handlers, and the inbox-
  triage copy had **drifted** — it omitted `<button>` and `contenteditable`, so
  a shortcut could fire mid-edit there. Extracted one shared guard
  (`ui/src/keyboard.rs` over a host-tested pure fn
  `common::keyboard::target_is_editable`) and reconciled all three call sites.
- The Cmd/Ctrl-K palette window listener (`layout.rs`) had no `on_cleanup`,
  leaking a handle that violates the project's listener-lifecycle rule; added
  the cleanup.

### Tooling — `scripts/preserve-ghcr-tags.sh` (GHCR image-tag archival)
One-time helper to copy release image tags from the old `jchultarsky101` GHCR
namespace (pre-2026-07 transfer) into the new `jchultarsky` one before the old
packages are deleted — registry-to-registry, idempotent, resumable. Optional
`TAG_FILTER` (ERE) restricts the range and `DRY_RUN=1` previews without
copying; the summary refuses to say "safe to delete" while any tags remain
filtered-out and old-namespace-only. Requires a `write:packages` PAT +
`docker login` (package writes are credential-scoped).

### Added — zero-AWS local login via bundled Keycloak (v3 groundwork)
`./scripts/dev-local.sh` brings up the full stack **plus a Keycloak OIDC
issuer** — no AWS account, Cognito pool, or secrets file needed to log in and
evaluate the app (the main OSS-adoption barrier). A protocol mapper emits
group membership under the same `cognito:groups` claim the app already reads,
so the security-critical token-validation path is **unchanged** — the only API
change is a guard on `/auth/change-password` (Cognito-only) that returns a
clean "managed by your identity provider" message against other issuers,
with a host-suffix detection helper + unit test. Realm fixture seeds `admin`
and `user` accounts (dev-only, not secrets). Verified end-to-end: login →
Keycloak → callback → `/api/auth/me` returns `roles:["admin"]` from the mapped
claim, and admin-only UI (Users/Permissions/Backup) renders. Files:
`deploy/keycloak/realm-ember-trove.json`, `deploy/docker-compose.local-auth.yml`,
`deploy/nginx.local-auth.conf`, `scripts/dev-local.sh`; README quickstart.

### Fixed — Keycloak local-login page now renders styled
The local-auth login page loaded unstyled: Keycloak's theme assets
(`/resources/**.css|.js|.ico`) were captured by the SPA's static-asset regex
location (`~* \.(css|js|ico…)$`), which nginx evaluates *before* the
`/resources/` proxy prefix, so they 404'd from the SPA root. Marked the
`/realms/` and `/resources/` proxy locations `^~` so the prefix wins over the
regex; all theme assets now serve 200 from Keycloak. Verified in a browser.

## [2.23.0] - 2026-07-17

### Tooling — coverage floor raised 17% → 24%
The v2.23.0 test work lifted api+common line coverage from ~18.7% to 25.96%;
the CI floor follows it up (same ~2-point margin as the original gate), so
the gains are now protected against regression.

### Added — palette commands for Search and Webhooks
The command palette is the primary navigation surface (since `/` opens it),
but `/search` had no Go-command — the 2026-07-17 review flagged the full
search page (presets, filters, full-text results) as near-undiscoverable.
Decision recorded in `.claude/ROADMAP.md`: keep the page, close the parity
gap. Adds `Go to Search` and `Go to Webhooks` palette commands with synonym
keywords; e2e-covered in `palette.spec.ts`.

### Added — webhooks management UI (`/webhooks`)
The webhooks backend (complete and SSRF-hardened since its introduction) was
headless — no UI called it. New sidebar entry + view: list with per-hook
Active/Paused toggle and signed indicator, create/edit form with per-event
checkboxes and HTTPS endpoint field, delete via the standard confirm modal.
Server validation errors (e.g. SSRF-blocked URLs) surface as toasts.
Covered by `e2e/tests/webhooks.spec.ts`.

### Fixed — webhook updates no longer wipe the stored secret
`PUT /webhooks/{id}` wrote `secret` unconditionally, and clients only ever
see the masked secret — so any UI edit (even an Active toggle) would have
silently cleared or corrupted the signing secret. `UpdateWebhookRequest.secret`
now uses the `deser_double_opt` PATCH pattern (absent → keep, null → clear,
value → replace) and the repo SQL only touches the column when the field was
present. Serde regression tests pin all three cases; the e2e spec verifies a
toggle leaves the secret in place.

### Tooling — repo-layer tests against real PostgreSQL (v2.23.0)
The repo layer's SQL was previously verified by nothing (stub-only router
tests). New `pg-tests` cargo feature gates `api/src/repo/pg_tests.rs`:
`#[sqlx::test]` gives each test its own freshly-migrated database. First
four tests pin node owner-scoping (`list_all_for_owner` vs `list_all`),
share-token expiry filtering, the node-scoped revoke SQL from the v2.22.4
security fix, and the task soft-delete → restore → purge lifecycle. New CI
job `repo tests (postgres)` runs them against the same postgres:16 service
the migration check uses; the default `cargo test` run stays database-free.

### Security — share-token and webhook-dispatch hardening (2026-07-17 review)
Three findings from the full-codebase security review, each with a regression test:
- `/share/{token}` now sits inside the rate-limited router group. It was the
  only unauthenticated endpoint outside the governor — and the one that
  performs a token lookup.
- `DELETE /nodes/{id}/share/{token_id}` scopes the revocation to the node in
  the path (`WHERE id = $1 AND node_id = $2`). Previously the repo deleted by
  token id alone, so the owner of any node could revoke any share token.
- Webhook delivery re-resolves and re-vets the target host at dispatch time
  and pins the HTTP client to the vetted addresses
  (`reqwest resolve_to_addrs`), closing the DNS-rebinding TOCTOU left by
  create/update-time-only SSRF validation. The SSRF guards moved to a shared
  `api/src/ssrf.rs` used by both validation and dispatch.

### Tooling — tests for the six previously-untested route groups (v2.23.0 start)
admin, backup, metrics, export, attachments, and editor-prefs — the most
privileged surfaces — had no tests at all (2026-07-17 review finding).
Added registration tests for all 15 routes plus behavior tests: non-admin
403s on admin/backup/metrics (including backup restore), export ZIP
owner-scoping (non-admin gets only their nodes; admin gets all), attachment
404s on unknown ids, and editor-prefs validation (entity-kind allowlist,
height clamp → 422). Test stubs now model canned nodes/attachments instead
of `unimplemented!` where these paths need them. Suite: 91 → 110 api tests.

### Tooling — e2e coverage for the graph view (v2.23.0)
The single largest UI surface (graph_view.rs, ~2.4k lines) had no e2e specs.
`graph.spec.ts` adds four: canvas rendering of created nodes, double-click
navigation to the node page, the full Add-Edge flow (source/target selection →
New Edge dialog → server-verified edge), and the orphans-only lens. Node
groups now carry `data-node-id` as a stable selector hook. Canvas
interactions use `dispatchEvent` — Playwright's positional clicks hang on
SVG actionability checks (recorded in `.claude/rules/e2e.md`).

### Documentation — open-source community health files
Added the standard community set for the now-intentionally-public repo:
`SECURITY.md` (private vulnerability reporting, scope, supported versions),
`CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), issue templates (bug/feature,
with a security-report redirect) and a PR template mirroring the gates.
Declared `license = "MIT"` in all three crate manifests and corrected the
stale "no LICENSE in the repo" comments (the MIT `LICENSE` has existed since
the repo went public); `CONTRIBUTING.md` now links the new files and explains
the fork → `develop` flow for external contributors.

## [2.22.3] - 2026-07-16

### Security — patched ammonia mXSS and quinn-proto DoS advisories
- `ammonia` 4.1.2 → 4.1.3 (RUSTSEC-2026-0193): mXSS bypass via MathML
  `annotation-xml` encoding strip. Directly relevant — ammonia sanitizes all
  user-supplied markdown before render, so the bypass was a stored-XSS vector.
  Brings the html5ever 0.39 transitive chain along (semver-compatible).
- `quinn-proto` 0.11.14 → 0.11.16 (RUSTSEC-2026-0185, high 7.5): remote memory
  exhaustion via unbounded out-of-order stream reassembly (transitive dep).

### Changed — GitHub repository ownership moved to `jchultarsky`
The repo moved from `jchultarsky101/ember-trove` to `jchultarsky/ember-trove`.
Repointed all owner-pinned references: the prod/k8s GHCR image paths
(`ghcr.io/jchultarsky/ember-trove-{api,ui}`, previously pinned to the old owner
while the release workflow — which uses `${{ github.repository_owner }}` — now
publishes under the new one), the `repository` fields in `api`/`ui` `Cargo.toml`,
the README build-status badge, and the clone/tag URLs in docs. GitHub's rename
redirect keeps old URLs working, but the shields.io badge does not follow it.

### Operations — TLS auto-renewal repaired + documented
Production HTTPS cert expired (2026-06-17) after ~30 days of silently failed
auto-renewals: the host's certbot renewal config used `authenticator =
standalone`, which can't bind port 80 because the nginx proxy container owns
it. Switched renewal to the `webroot` method (the architecture nginx was
already built for), reissued the cert, and replaced the post-renewal nginx
reload hook with a Docker-direct version (no compose/env dependency). Proven
end to end with `certbot renew --dry-run`. The renewal setup — which lived
only on the host and had no repo record — is now captured in
`deploy/TLS-RENEWAL.md` (+ a version-controlled `deploy/renewal-hooks/reload-nginx.sh`).


## [2.22.2] - 2026-06-10

### Documentation — ROADMAP updates (v2.22.0 prod verification, AuthGate fix record)
Docs-only release; also serves as the live verification vehicle for the
v2.22.1 AuthGate deploy-resilience fix (an open tab should survive this
deploy's restart window).

## [2.22.1] - 2026-06-10

### Fixed — Deploys no longer force open tabs to re-login
`AuthGate` treated any failed `/api/auth/me` probe as "not logged in" and
immediately bounced the tab to Cognito — so the few seconds of API downtime
during a deploy restart logged every open tab out despite a still-valid
session cookie. The probe now retries transient failures (network errors,
5xx) with backoff for ~23 s before giving up; only an authoritative 401/403
ends the session. Diagnosed after three forced re-logins during the
2026-06-10 release train.

## [2.22.0] - 2026-06-10

### Added — My Day: carryover prompt and foldable overdue section
Carried-over tasks (focused on a previous day, still open) now ask
"still today?" inline: **Yes** re-stamps the focus date to today (clearing
the badge), **No** drops the task back to the backlog — making the morning
carry-over decision one click instead of silent stickiness. Overdue tasks
get their own foldable "Overdue · N" section above the backlog (red accent,
expanded by default) instead of mixing into the top of the list — visible,
but never a pinned guilt pile. Keyboard j/k order follows the display order
and skips folded overdue rows. The binary `focus_date` model is unchanged
(see the 2026-04-28 ADR).

### Tooling — E2E coverage for saved search presets
The presets UI (save current filters under a name, load, delete) already
existed but was untested and mislisted as missing in the ROADMAP backlog —
now pinned by an e2e spec (save → reload-persist → load applies the query →
delete).

### Added — Calendar quick-add
Clicking a day cell on the Calendar opens an inline composer; Enter creates a
standalone task due that day (it lands in the Inbox for triage like any other
capture), Escape cancels. Task chips still navigate to their node — clicks no
longer fall through to the cell. Day cells carry `data-date` attributes.

### Changed — Focus traps on the remaining modals
Create-node and add-favorite now trap Tab focus, return focus on close, and
carry `role="dialog"`/`aria-modal` — completing the modal a11y pass started
in v2.21.0 (quick capture, palette, delete confirm, help).

## [2.21.4] - 2026-06-10

### Fixed — Palette: commands no longer hijacked by body-text node matches
Typing a command-intent query like "theme" or "dark" could rank nodes whose
*bodies* mention the word above the command itself, so Enter opened a node
instead of running the command (found live-testing v2.21.3). The non-empty
query list is now ordered: title-matched nodes (the quick-switcher core) →
commands → body-only node matches → Create. Unit-tested (`ranked_actions`)
and pinned by an e2e regression with a bait node (14 specs total).

## [2.21.3] - 2026-06-10

### Tooling — E2E specs for inbox triage and the command palette
Eight new Playwright specs (13 total): triage `t`/`s`/`a` decisions with
API-verified server state, skip-wrapping and no-changes exit; palette
open/close, synonym command matching ("theme" finds dark mode), navigation
dispatch, dark-mode round-trip, node search→open, and node-context commands.
Triage specs clear the inbox up front (the working set is a mount-time
snapshot) and the smoke capture spec now cleans up its task. The triage card
gained a `data-testid` — task titles also exist in the CSS-hidden list behind
it, which strict mode rightly rejects; lesson recorded in
`.claude/rules/e2e.md`.

## [2.21.2] - 2026-06-10

### Tooling — Playwright e2e smoke suite
Browser-level smoke tests (`e2e/`, `scripts/e2e.sh`, CI job `e2e`) — the
direct response to v2.21.1, where both hotfix bugs were structurally
invisible to `cargo test`/clippy. Five specs: app shell + route title,
NL quick-capture (chips → inbox), soft delete → undo toast → restore,
the zombie-window-listener regression, and editor autosave (indicator +
server state). The suite runs against a dedicated Docker stack
(`deploy/docker-compose.e2e.yml`, compose project `ember-e2e`, tmpfs
Postgres) whose api is built with the new `e2e-bypass` cargo feature —
auth is short-circuited to a synthetic non-admin user. **Security:** the
release Dockerfile builds without features, so the bypass code path never
exists in shipped binaries, and even a feature-built binary requires
`E2E_AUTH_BYPASS=1` at runtime. No local Node needed: Playwright runs in
its official Docker image (`E2E_RUNNER=native` opts into local node).
Also: optional env vars (OIDC/S3) now treat empty strings as unset so
compose overrides can disable them.

## [2.21.1] - 2026-06-10

### Fixed — Zombie keyboard listener panics (hotfix)
`MyDayView` discarded its `window_event_listener` handle behind a comment
claiming Leptos auto-removes it on unmount — it does not. Every unmounted
My Day instance left a zombie keydown listener that read disposed signals on
the next keypress, panicked the WASM runtime, and poisoned all event dispatch
(quick capture and toasts went dead until reload). The handle is now removed
in `on_cleanup`, matching the layout.rs convention.

### Fixed — Undo toasts (and other post-await toasts) never rendered
`push_toast`/`push_undo_toast` resolved `ToastState` via `use_context`, which
returns `None` in code resumed after an `.await` inside
`wasm_bindgen_futures::spawn_local` (no reactive owner) — so v2.21.0's undo
toasts, and several older success/error toasts pushed from API continuations,
were silently dropped. The helpers now fall back to a thread-local global
handle set when the app creates its `ToastState`.

## [2.21.0] - 2026-06-10

### Added — Local graph on node pages; orphans lens on the global graph
- Every node page gains a collapsible **Local Graph** panel: the node and its
  direct connections (outgoing edges + backlinks, deduped) in a small radial
  map. Click a neighbor to navigate; beyond 12 connections a "+N more" hint
  links to the full graph. Loads lazily on first open.
- The global graph's legend gains an **Orphans only** toggle: show just the
  nodes with no links anywhere — a maintenance lens for finding notes that
  never got connected. (Type and tag filters already existed.)

### Changed — Accessibility pass (SPA fundamentals)
- **Modals** (quick capture, command palette, delete confirm, help) now trap
  `Tab` focus inside the dialog and return focus to the triggering element on
  close; all carry `role="dialog"`/`"alertdialog"` + `aria-modal`. The delete
  confirmation autofocuses Cancel (the safe action) and closes on `Escape`.
- **Route changes** update `document.title` ("My Day — Ember Trove", …) and
  move focus to the main region so screen readers announce navigation.
- **Toasts** are a `role="status"` polite live region — save/delete/undo
  outcomes are announced.
- **Task tabs** (My Day / Inbox / Calendar) follow the ARIA tabs pattern:
  roving tabindex and left/right arrow keys switch tabs.
- **Priority dots** carry `title`/`aria-label` ("High priority") instead of
  being color-only.

### Added — Command palette actions
The Cmd-K palette now runs commands, not just node search: navigation
("Go to My Day / Inbox / Calendar / Dashboard / Graph / Notes / All Nodes /
Tags / Templates"), "New task (quick capture)", "New node…", "Toggle dark
mode", and "Help & shortcuts" — each matched against synonyms ("theme" finds
dark mode) and showing its global shortcut in the row so the palette teaches
the keyboard layer. On a node page, "Edit current node" and "Duplicate
current node" join the list. The empty-query view shows Recent plus the two
capture commands.

### Added — Keyboard inbox triage ("Process" mode)
A **Process** button on the Inbox opens a one-task-at-a-time triage card:
`t` adds to today, `s` schedules a due date, `a` attaches to a node (with the
debounced picker), `d` deletes (undoable), `j`/`k` skip/go back, `Esc` exits.
Handled tasks leave the working set; skipped ones come around again. The
working set is a snapshot, so nothing shifts mid-flow; the inbox refetches
once on exit. Shortcuts are documented in the Help modal.

### Added — Natural-language quick add
The quick-capture box (`n`) now parses date and priority tokens from the
first line: `buy milk friday p1` captures "buy milk" due next Friday at high
priority. Supported: `today`, `tomorrow`/`tmrw`, weekday names (`fri`,
`friday`, …), ISO dates (`2026-07-01`), and `p1`/`p2`/`p3` or
`!high`/`!medium`/`!low`. Chips under the box preview the interpretation live
before you submit; later lines (shared URLs, pasted text) are never scanned,
last token per category wins, and an input that is *only* tokens stays a
plain title. Parser: `common::quickadd` (unit-tested); the wire format gains
optional `due_date`/`priority` on `POST /api/inbox/quick` (older clients —
the iOS share sheet — are unaffected).

### Added — Unlinked mentions under "Linked Here"
The backlinks panel on a node page now lists **Mentions**: nodes whose text
contains this node's title without linking to it (full-text matches, minus
existing backlinks). Each row has a one-click **Link** action that rewrites
the first plain-text occurrence in the mentioning node into a wikilink —
prose casing is preserved via the alias form (`[[Title|prose text]]`), word
boundaries are respected, and text already inside `[[...]]` is never touched
(`common::markdown::link_first_mention`, unit-tested). The rewrite goes
through the normal node update path, so it versions, syncs wikilinks (the
mention immediately becomes a backlink), and respects edit permissions.

### Changed — Loading & theme polish
- Search results and the Templates gallery now show content-shaped skeletons
  while loading instead of bare "Loading…" text.
- With no stored theme preference, the app follows the OS
  (`prefers-color-scheme`) instead of defaulting to light — first paint (the
  static loading screen already honored the media query) and the app now agree.

### Added — Undo for task & note deletion (soft delete)
Tasks and notes previously hard-deleted instantly — including via the My Day
`d` shortcut — with no confirmation and no way back, while nodes/tags got
confirm modals. Deletion now follows the undo-toast pattern (instant action,
recoverable) instead of interrupting with dialogs:

- **API**: `DELETE` on tasks/notes tombstones the row (`deleted_at`,
  migration 030) instead of erasing it; new `POST /tasks/:id/restore` and
  `POST /notes/:id/restore` endpoints un-delete (authorization mirrors
  delete). Every live query — lists, feeds, dashboards, counts, calendar,
  backups — filters tombstones out. Tombstones older than 30 days are purged
  at API startup and daily thereafter.
- **UI**: every task/note delete (My Day rows + `d` key, Inbox cards, the
  node task panel, the notes feed) shows a "Task/Note deleted — Undo" toast
  for 8 s; Undo restores the item in place. "Clear all completed" gets a
  bulk undo ("Deleted N completed tasks — Undo"). The notes feed's inline
  are-you-sure confirmation is gone — delete is instant and recoverable.
  Node deletion keeps its specific confirm modal (nodes cascade to their
  tasks/notes/attachments, so a dialog is still warranted there).

### Added — Editor autosave & unsaved-work protection
The node editor could silently lose work: no autosave, no dirty tracking, and the
global `Escape` handler (or any sidebar click / tab close) discarded everything
since the last manual Save. Now:

- **Edit mode autosaves**: changes PATCH automatically 2 s after typing pauses
  (debounced; in-flight saves coalesce, with a recheck after each round-trip).
  Navigating away flushes any still-unsaved edits as the editor unmounts, and
  closing/refreshing the tab while dirty triggers the browser's native
  unsaved-changes prompt. Failed autosaves keep the edits locally and retry on
  the next change instead of hammering the API.
- **Create mode keeps a local draft**: `/nodes/new` content (title, body, type,
  status) persists to `localStorage` as you type, is restored on the next visit,
  and is cleared on successful create. A pristine scaffold is never persisted.
- **Save-state indicator** in the editor header ("Unsaved changes… / Saving… /
  Saved / Draft kept locally / Couldn't save — edits kept here"), `aria-live`
  so screen readers hear save outcomes. The manual Save button still works and
  navigates to the node view as before.

### Fixed — Mutations no longer fail silently (optimistic-rollback sweep)
A sweep of every `let _ = crate::api::…` fire-and-forget mutation in the UI
(18 sites). Previously a failed PATCH/DELETE after an optimistic update left
the screen showing success while the server kept the old state — no toast, no
rollback. Now every mutation surfaces an error toast on failure; optimistic
flips revert (task done-checkbox in My Day/Inbox/node task panel, My Day
add/remove, inline title/priority edits); task reordering refetches to restore
the server's order; "Clear all completed" reports how many deletes failed; and
note, edge, tag-detach, and node-link removals report errors. List refetches
now run only on success, so a dead network no longer triggers refetch storms.
The four `set_editor_pref` height-preference writes stay deliberately
fire-and-forget (cosmetic) and are annotated as such.

### Fixed — Failed node load can no longer be saved back as an empty node
In edit mode, if the initial `fetch_node` failed, the editor showed empty fields
with Save enabled — clicking it would overwrite the real node with an empty body.
A load failure now shows an error banner and keeps Save (and autosave) disabled.

### Changed — Version snapshots dedupe; "Edited" activity coalesces
With autosave PATCHing every few seconds, two server-side behaviors needed tuning
(both also improve manual saves):

- `PUT /nodes/:id` skips the `node_versions` snapshot when the body is unchanged
  from the latest stored version (title/status-only saves previously inserted
  pure duplicates). Snapshots are now awaited inline rather than spawned, so
  rapid saves can't record versions out of order.
- Consecutive `Edited` activity entries by the same user on the same node within
  15 minutes coalesce into one, so the dashboard recap shows one line per editing
  session instead of one per autosave.

## [2.20.4] - 2026-06-06

### Documentation — Rewrite local-dev docs for the Cognito reality (remove stale Keycloak)
Auth moved from Keycloak to Cognito long ago (`feat(auth): replace Keycloak with Amazon
Cognito`), but the README's "Local Development" walkthrough still described a Keycloak
setup that no longer exists in the compose — so following it failed. Fully rewrote it
(`README.md` → "Running Locally"): an **Option A — Full Docker stack** flow (verified:
`cp .env.local.example`, fill secrets, `--env-file deploy/.env.local up --build`,
`localhost:8003`) and an **Option B — native** flow (cargo + `trunk serve`, which proxies
`/api`), plus a **"bring your own Cognito"** section so a fresh cloner can stand up their
own user pool + app client + callback. Also: corrected the `COOKIE_KEY` guidance (the
all-zeros value is rejected, not "safe for local dev"), dropped dead env vars from the
config reference (`OIDC_EXTERNAL_URL`, `KEYCLOAK_ADMIN_*`), fixed the Key Features / Tech
Stack auth rows, added the `/api/auth/change-password` endpoint, and updated
`CONTRIBUTING.md`'s local-stack snippet. Known follow-up: local login still requires a
Cognito pool (no bundled local IdP) — documented as such.

### Tooling — Dependency bump
`taiki-e/install-action` 2.81.5 → 2.81.6 (Dependabot, github-actions group).

### Tooling — Local Docker stack: require a real COOKIE_KEY (was broken out of the box)
The local `deploy/docker-compose.yml` hardcoded an all-zeros `COOKIE_KEY`, which
the API correctly rejects at startup ("trivially weak") — so the api container
crash-looped and the local stack never came up. `COOKIE_KEY` is now required from
`deploy/.env.local` (`${COOKIE_KEY:?…}`), documented in `.env.local.example` with
`openssl rand -hex 64`. Verified the full stack (postgres + MinIO + api + ui/nginx)
boots and serves at `http://localhost:8003`.

### Tooling — Stop the recurring red `chore(deps)` CI from the rand 0.10 bump
Dependabot opened a weekly `rand` 0.9 → 0.10 PR whose CI failed every time:
rand 0.10 has a breaking `RngCore`/`fill_bytes` API change, and — more
fundamentally — `governor 0.10` (via `tower_governor 0.8`) still pins `rand ^0.9`,
so adopting 0.10 would force two `rand` versions into the tree in the CSPRNG path
that mints PKCE verifiers and OAuth state. Added a dependabot `ignore` for
`rand` `0.10.x` (with a dated rationale) so the bump is deferred until the
ecosystem supports a single-version migration. Closed the stale PR.

## [2.20.3] - 2026-06-05

### Changed — Loosen auth rate limits for active development
The strict `auth` nginx zone (login/callback/refresh/logout) was 10 r/m, burst 5.
During iterative login testing that throttled quickly, and — worse — `/api/auth/me`
(the session check hit on *every* app load) shared that budget: once it 429'd, the
SPA's AuthGate treated it as logged-out and looped login↔callback, re-saturating
the limiter. Two changes in `deploy/nginx.prod.conf`: (1) `/api/auth/me` now has
its own generous zone (120 r/m) via an exact-match `location =` so a session check
never trips the brute-force limiter; (2) the `auth` zone is loosened to 30 r/m,
burst 10. Deliberately relaxed for the active-development phase — revisit and
tighten later. Validated with `nginx -t`.

### Security/Tooling — Remove committed test private key; add a pre-commit secret scan
The 2.20.2 JWT regression test embedded a generated RSA **private** key (used only
to sign a test token). It was a throwaway with no production value — it never
protected real data and nothing needs rotation — but committing a `BEGIN … PRIVATE
KEY` block is wrong and tripped secret scanning. The test now uses only the
**public** key and drives the verification path with a bogus-signature token (still
reproduces the original panic without a crypto backend). Added a zero-dependency
secret scan to `scripts/hooks/pre-commit` that blocks staged private-key/AWS-key
material, and a normative "never commit private keys, even in tests" rule to
`.claude/POLICY.md` §1.

## [2.20.2] - 2026-06-05

### Fixed — Login still broken: JWT validation panicked → 502 on every authenticated request
A second 2.20.0 regression, exposed once the 2.20.1 callback fix let the flow
reach the API: every authenticated request (`/api/auth/me`, etc.) returned
**502** because the worker **panicked** validating the session JWT. Root cause:
the `jsonwebtoken` 9 → 10 bump. v10 ships **no built-in crypto backend** (v9
bundled `ring`); a backend feature must be enabled explicitly or the first RS256
verification panics — *"Could not automatically determine the process-level
CryptoProvider from jsonwebtoken crate features."* Our manifest enabled neither,
so login could never complete. Enabled the **`aws_lc_rs`** backend (the provider
rustls/the AWS SDK already use, so the tree stays single-crypto). Added an RS256
sign+verify regression test in `auth::oidc` — the existing middleware tests run
with `oidc = None` and never exercised the verify path, which is why CI stayed
green while prod crashed.
- **Security:** chose `aws_lc_rs` over `rust_crypto` deliberately — `rust_crypto`
  would pull the pure-Rust `rsa` crate (RUSTSEC-2023-0071, Marvin timing). `cargo
  audit` remains clean and no `rsa` crate enters the tree.

## [2.20.1] - 2026-06-05

### Fixed — Login left users on a blank "Redirecting…" page (CSP vs. inline script)
After login, Cognito redirects the browser to `/api/auth/callback`, which set the
session cookie and then bounced into the SPA via an **inline** `<script>
window.location.replace(…)</script>`. The 2.20.0 deploy added a strict
Content-Security-Policy (`deploy/nginx.prod.conf`) whose `script-src` uses a
per-request nonce + `'strict-dynamic'` with no `'unsafe-inline'`. The nonce is
stamped onto `<script>` tags by an nginx `sub_filter` that runs **only** in the
frontend `location /` block — not `location ~ ^/api/auth/` — so the callback's
inline redirect script carried no nonce and CSP blocked it. The redirect never
fired and the user was stranded on a blank page. The callback now issues a real
**HTTP 303 redirect** (`axum::response::Redirect`) instead of an inline script,
so it needs no nonce and the strict CSP stays fully intact (no `'unsafe-inline'`
re-introduced). Session/refresh/access cookies are unchanged.

## [2.20.0] - 2026-06-05

### Tooling — CI hardening (coverage floor + cargo-deny)
- The coverage job is now a **hard gate**: `cargo llvm-cov … --fail-under-lines 17`
  (line-coverage baseline ~18.7% on 2026-06-05). The floor is set below the baseline
  so it never blocks the current suite but does catch a regression; raise it as the
  suite grows.
- Added **`cargo-deny`** (`deny.toml` + a new CI job) for the supply-chain checks
  `cargo audit` does *not* perform: **licenses + bans + sources**. RUSTSEC advisories
  remain solely with `cargo audit` (`.cargo/audit.toml` stays the single source of
  truth), so the two gates don't overlap — the reason cargo-deny had been deferred.
  Workspace crates are marked `publish = false` (they're a self-hosted app, never
  published) and skipped via `[licenses].private`; three permissive transitive
  licenses (`BSL-1.0`, `CDLA-Permissive-2.0`, `bzip2-1.0.6`) are allow-listed with
  provenance comments.

### Added — Notes feed "Load more" paging
The Notes feed previously requested a single `per_page=1000` page (a hard
truncation past 1000 notes). It now pages: each fetch pulls `FEED_PAGE_SIZE`
(50) rows and a **Load more** button appends the next page until the feed is
exhausted. The server already supported `page`/`per_page` with `LIMIT/OFFSET`;
this is a UI change. A version-guarded fresh-load effect (the project's debounce
pattern) resets to page 1 and replaces the list when any filter/sort changes, so
a stale in-flight fetch can't clobber a newer one; "more available" is inferred
from a full page coming back. Removes the standing `notes.rs` TODO.

### Added — Webhook delivery is now wired (registered webhooks actually fire)
The `webhook_dispatch::dispatch` delivery path existed (tenant-scoped,
SSRF-hardened, HMAC-signed) but had **no callers**, so registered webhooks
never fired. It is now invoked from the node and task CRUD handlers:
`node.created` / `node.updated` / `node.deleted` (`routes/nodes.rs`) and
`task.created` / `task.updated` / `task.deleted` (`routes/tasks.rs`). Delivery
is scoped to the *resource owner's* own active webhooks (a node/task event only
fans out to the owner's endpoints — never cross-tenant). Event names are now
canonical `pub const`s + an `available_events()` allowlist in `common::webhook`
(used by `default_events`), so handlers and clients can't drift on spelling.
`task.*` joins `node.*` as a subscribable event; `default_events()` now
subscribes to all six. The crate-wide dead-code lint is satisfied without the
prior module-scoped `allow` (dispatch has real callers now).

> Behaviour note: deployments with webhooks that were registered while delivery
> was dormant will start receiving POSTs after this upgrade — that is the
> intended fix, but call it out in release notes.

### Tooling
- Dependency major bumps (resolving the remaining open Dependabot PRs):
  `reqwest` 0.12 → 0.13 (added the now-feature-gated `form` feature for the OIDC
  token/revocation calls; 0.13 also defaults `default-tls` to the modern rustls
  stack, consistent with our AWS-SDK TLS posture), `axum-extra` 0.10 → 0.12,
  `rand` 0.8 → 0.9 (`thread_rng()` → `rng()`), and `ammonia` 3 → 4 (markdown
  sanitizer; API-compatible).
- **Unblocked the deferred `sha2`/`hmac` bump.** `sha2` 0.10 → 0.11 and `hmac`
  0.12 → 0.13 are now bumped *together*, so both ride the same `digest` 0.11 and
  `Hmac<Sha256>` again satisfies `hmac::Mac` (the earlier single-crate bump broke
  because `hmac` 0.12 was still on `digest` 0.10 — see prior `[Unreleased]` note).
  `new_from_slice` moved to the `KeyInit` trait, now imported in
  `webhook_dispatch.rs`. The webhook HMAC-signing and PKCE S256 paths are
  unchanged in behaviour.

### Removed
- Deleted the orphaned `ui/src/components/modals/link_picker.rs` "Phase 4" stub
  (`#![allow(dead_code)]`, zero call sites, UI text still read "Link picker coming
  in Phase 4"). Node-to-node linking has shipped via the graph view's inline edge
  creation (`POST /edges`) since Phase 4, making the modal dead. Also removed two
  orphaned task-sort helpers in `ui/src/components/task_common.rs`
  (`sort_tasks_full` + its sole consumer `priority_weight`, superseded by
  `sort_tasks_by_order`) and the never-mounted `LegendShape` graph-legend
  component (the edge legend `LegendEdge` remains in use).

### Changed
- Dropped the crate-wide `#![allow(dead_code)]` (with its stale "Phase 1 skeleton"
  rationale) from `api/src/main.rs` and `ui/src/main.rs`, restoring real dead-code
  lint coverage across both binaries. Genuinely-retained dead code is now scoped
  with localized `#[allow(dead_code)]` + justification instead of a blanket
  relaxation: the sqlx-deser-only `SummaryRow.rn` / `SearchRow.updated_at` fields,
  the deliberate `IconButtonVariant::Accent` palette variant, and the
  not-yet-wired webhook delivery module (`api/src/webhook_dispatch.rs`).

### Tooling
- Bumped `actions/cache` from v4 to v5.0.5 (SHA-pinned, `# v5.0.5`) across all CI jobs
  to move off the deprecated Node.js 20 runtime (GitHub forces Node 20 actions onto
  Node 24; v5 targets Node 24 natively). Clears the CI deprecation annotation.
- Dependency bumps (consolidating Dependabot #5, #6, #8, #9 into one resolved lockfile):
  `jsonwebtoken` 9 → 10 (OIDC JWT verification; our `Validation` config is explicit —
  RS256-pinned, audience + issuer pinned, `exp` enforced — so the major bump is
  behaviour-preserving), `zip` 3 → 8 (backup/export), `gloo-net` 0.6 → 0.7 (UI HTTP),
  `leptos` 0.8.16 → 0.8.19, plus the weekly minor/patch group (`tokio`, `uuid`,
  `wasm-bindgen`, `web-sys`, `tower-http`, `utoipa`, … ). `sha2` 0.11 (Dependabot #7)
  is **deferred**: it pulls `digest` 0.11 while `hmac` is still on `digest` 0.10, so
  `HmacCore<Sha256>` stops satisfying `hmac::Mac` and the build breaks — revisit once the
  RustCrypto `digest` 0.11 ecosystem (incl. `hmac`) is released.

### Documentation
- Corrected `CLAUDE.md`'s stale environment note: `gh` is now installed (Homebrew) and
  authenticated, and is the supported way to push from tool shells (the sandbox keychain
  does not unlock non-interactively).

## [2.19.3] - 2026-06-05

### Tooling
- Enforced the zero-panic policy as lints instead of leaving it to review:
  `clippy::unwrap_used`, `expect_used`, and `panic` are denied in
  `[workspace.lints.clippy]` (each crate opts in with `[lints] workspace = true`),
  with test code exempted via `clippy.toml` (`allow-*-in-tests`). The clippy gate
  now fails on a stray panic path; previously the rule was documented but unchecked
  (`-D warnings` does not enable the restriction lints by default).
- Made `scripts/verify.sh` pass `--workspace --exclude ui` explicitly (matching CI
  and the git hooks) rather than relying on `default-members`.
- Adopted default `rustfmt` (edition 2024) as an enforced standard: one-time
  workspace reformat, plus `cargo fmt --all --check` in the pre-commit hook, CI
  (`fmt` job), and `scripts/verify.sh`. CI previously never checked formatting.
- Added version-controlled git hooks (`scripts/hooks/`, installed via
  `make hooks-install`): pre-commit runs fmt + clippy; pre-push runs the test
  suite + the wasm32 UI lint.
- Added a `Makefile` with `hooks-install`, `fmt`, `lint`, `test`, `coverage`,
  and `verify` targets wrapping the canonical commands.
- Added a CI `coverage` job (`cargo llvm-cov` over api + common, report-only).
- Pinned all third-party GitHub Actions to commit SHAs and added Dependabot
  (`cargo` + `github-actions`, weekly, targeting `develop`).
- Declared `rustfmt` in `rust-toolchain.toml` components (required by the CI fmt job).

### Changed
- Migrated all `ui/` required-context lookups from `use_context::<T>().expect(..)`
  to the idiomatic `expect_context::<T>()` (behaviour-identical; satisfies the new
  `expect_used` lint, which treats a missing provider as a wiring invariant rather
  than a policed panic). Replaced one infallible `and_hms_opt(0,0,0).unwrap()` in
  `project_dashboard.rs` with `and_time(NaiveTime::MIN)`.

### Documentation
- Documented the `expect_context` carve-out and the lint enforcement in
  `.claude/POLICY.md` §3, `.claude/rules/leptos.md`, and `CLAUDE.md`.
- Added a `.claude/` knowledge tree (`POLICY.md`, `ERRORS.md`, `ROADMAP.md`,
  `rules/`, `patterns/`) that the previously-dangling `CLAUDE.md` references now
  resolve to, and slimmed `CLAUDE.md` to a lean always-loaded core.
- Documented the development workflow/standards in `README.md` and aligned
  `CONTRIBUTING.md` with the enforced hooks and formatting.

### Fixed
- `ui/src/components/node_list.rs`: replaced two sort closures with `sort_by_key`
  (clippy `unnecessary_sort_by`, surfaced when rustfmt collapsed the block bodies).

## [2.19.2] - 2026-05-31

### Security — Removed `script-src 'unsafe-inline'` from the CSP (last review finding)
The edge CSP's `script-src` no longer carries `'unsafe-inline'` — it uses a
per-request nonce plus `'strict-dynamic'`. The reverse proxy injects the
request's `$request_id` into every `<script>` tag (`sub_filter`) and emits the
matching `nonce-…` in the header, so the inline service-worker registration,
the `WebAssembly.instantiateStreaming` shim, and Trunk's module bootstrap run
without `'unsafe-inline'`; `'strict-dynamic'` lets the nonced bootstrap load
the per-build, content-hashed WASM-glue JS it imports (no fragile per-build
hash pinning). `'wasm-unsafe-eval'` and `'unsafe-eval'` remain (required by
wasm-bindgen's glue), and `style-src` keeps `'unsafe-inline'` for the
loading-screen `<style>` and Tailwind. Injected inline scripts (e.g. via an
ammonia-sanitizer bypass) can no longer execute. This closes the final
outstanding item from the security reviews/audit (sprints 7–9).

## [2.19.1] - 2026-05-31

Security sprint 9 — hardening for the blind spots flagged by the deep audit's
completeness critic (all Low). The same pass confirmed activity-log read
scoping, backup/restore authorization, rate-limiting coverage, and a full
sqlx-parameterization sweep are **clean** (no findings).

### Security — Cognito admin client no longer leaks provider internals
Every `CognitoAdminClient` operation wrapped the raw AWS/Cognito error string
into the client-facing response, leaking the User Pool ID and internal
exception types (notably `UsernameExistsException`, which re-opened a
user-enumeration oracle via the invite endpoint). Errors now map to a generic
message with the detail logged server-side. The `find_user_by_email`
`ListUsers` filter also escapes `\` and `"` (defense-in-depth, independent of
the upstream `garde(email)` validation).

### Security — Backup restore is bounded against decompression bombs
The admin-only restore now caps per-entry (512 MiB) and cumulative (2 GiB)
extracted size, and rejects attachment archive paths containing `..` before
they are used as S3 keys.

## [2.19.0] - 2026-05-31

Security sprint 8 — follow-ups from a deep, adversarially-verified security
audit, plus the auth-flow items deferred from v2.18.0. (Still deferred: the
CSP `'unsafe-inline'`/`'unsafe-eval'` removal, which needs a nonce/hash on the
Trunk bootstrap and browser verification.)

### Security — Graph & edge endpoints are now tenant-scoped
The graph surface was the last endpoint family trusting client-side filtering:
- `PUT /graph/positions` (batch) had **no ownership check** — any authenticated
  user could overwrite the layout coordinates of *any* node, and the array was
  unbounded. The write is now owner-scoped (rows for nodes the caller doesn't
  own are dropped) and capped at 2000 entries.
- `GET /graph/positions` and `GET /edges` returned **every** node position /
  edge in the system (leaking other owners' node UUIDs). Both are now scoped to
  the caller's own nodes (admins still see all).

### Security — Node ownership can no longer be hijacked
A user granted `Owner` on a shared node could revoke or downgrade the canonical
creator's permission row, locking them out (`require_role` consults only the
permissions table). The creator's row (`nodes.owner_id`) is now immutable except
by an admin.

### Security — Authentication hardening
- The JWT **issuer** (`iss`) is now validated (`set_issuer`) in addition to the
  existing RS256 / audience / expiry checks, and an **access token presented in
  place of an ID token** on the session path is rejected (`token_use`).
- The OAuth **`state`** is now bound to the initiating browser via a short-lived,
  encrypted `SameSite=Lax` cookie that the callback must match — closing a
  login-CSRF / session-fixation gap.

### Security — Webhook SSRF & cross-tenant fan-out
- Webhook URLs are now DNS-resolved at registration and rejected if any resolved
  IP is private/link-local/IMDS; the dispatcher no longer follows redirects.
- Webhook delivery is filtered to the resource owner's own webhooks.

### Security — Hardening & cleanup
- Share-invite email HTML body now escapes user-controlled values (node title,
  inviter) to prevent HTML/phishing injection.
- `/api/auth/refresh` and change-password return generic error messages
  (upstream Cognito detail is logged, not reflected).
- Attachment upload now uses a content-type **allowlist** (was a bypassable
  blocklist); downloads already send `nosniff`.
- `CreateEdgeRequest.label` and search-preset fields are length-validated.
- `create_webhook` masks the secret in its response (consistent with list/update).
- `COOKIE_SECURE` now defaults to `true` (the dev compose opts out for http).
- Markdown `style` attributes are filtered to a safe CSS property allowlist
  (colour/emphasis kept; `position`/`z-index`/`url()` stripped) — removing the
  clickjacking-overlay vector while preserving inline colours.
- Removed the dead Keycloak realm seed file (contained hardcoded example
  passwords) and theme; the project is Cognito-only.

### Fixed
- Replaced three `.expect()` calls on the SES invite path with graceful skips
  (no-panic guarantee).

## [2.18.0] - 2026-05-31

Security sprint 7 — authorization hardening, from a comprehensive review
prompted by the growing personal-data surface. Fixes the low-deploy-risk
broken-access-control issues; login-flow, CSP, and rendering items
(JWT issuer validation, OAuth `state` CSRF binding, webhook SSRF, CSP
`unsafe-inline`, ammonia `style` CSS) are tracked as follow-ups.

### Security — Search results are now scoped to the caller
The global search endpoint extracted no auth claims and the query layer
had no owner predicate, so any authenticated user could read **every
other user's** node titles, note-body snippets, and task titles via
`GET /api/search`. `SearchRepo::search` now takes the caller's subject
and scopes the `valid_nodes` CTE / node query by `owner_id`; admins pass
`None` to search across all owners. (Was a system-wide read breach of the
most sensitive content; required no special access.)

### Security — Node-link authorization
- `GET /nodes/:id/links` now requires `require_viewer` (was an
  unauthenticated read — any user could enumerate any node's links).
- Node-link `update`/`delete` are now scoped by `node_id` as well as
  `link_id`, so a user with editor rights on one node can no longer
  mutate a link belonging to another node (IDOR).

### Security — Template reads scoped to the owner
`GET /templates` and `GET /templates/:id` returned any user's templates
(bodies are user-authored). Both are now owner-scoped (admins see all),
matching the existing write-path checks.

### Security — Notes feed is bounded + LIKE-safe
`GET /notes/feed` had no `LIMIT` (a full-table scan; the admin path
serialized every owner's notes, and `q=%` made it trivial). It is now
bounded by a server-clamped page size, and `%`/`_`/`\` in the text
filter are escaped so they match literally instead of as wildcards.

### Fixed — Webhook secret masking no longer panics on multibyte secrets
`mask_secret` byte-sliced `&s[..4]`, panicking when the 4th byte fell
mid-UTF-8. Truncation is now char-safe.

### Security — Attachment downloads send `X-Content-Type-Options: nosniff`
Defense-in-depth alongside the existing `Content-Disposition: attachment`
so a stored file can't be MIME-sniffed into an executable type.

## [2.17.1] - 2026-05-30

### Fixed — `require_role` preserves the structured error variant
`require_role` wrapped `PermissionRepo::find` in
`.map_err(|e| ApiError::Internal(e.to_string()))`, flattening every repo
error — including the structured `NotFound` / `Forbidden` variants — into
a generic `500 Internal`. The `?` operator already converts
`EmberTroveError` to `ApiError` via the `From` impl in `error.rs`, which
preserves the variant. Dropped the redundant `map_err` so permission
failures surface the correct status. No behaviour change on the success
path; internal error fidelity only.

## [2.17.0] - 2026-05-30

### Changed — Resizable, per-task-persisted task editing
Task title inline-edit moved from a single-line `<input>` to the shared
`ResizableEditor` (drag-resizable textarea) across My Day / Kanban
(`KanbanTaskRow`), the node `TaskPanel`, and the Inbox — save on
Ctrl/Cmd+Enter, cancel on Escape. The dragged height is persisted
per-task server-side via the `editor_prefs` table and re-applied on next
open through a `TaskEditorHeights` context map provided by each list view.
Long task titles can now be edited comfortably without a cramped field.

## [2.16.0] - 2026-05-30

### Added — Shared editor components + server-side editor preferences
- **`NodePicker`** — a debounced type-ahead node selector (extracted from
  the task form) replaces the parent-node `<select>` in the Notes compose
  box and is now shared by both the note and task forms.
- **`ResizableEditor`** — a shared resize-y textarea used by the feed
  compose box and node note create/edit, reporting its height on mouseup.
- **`editor_prefs` table** (migration `029_editor_prefs.sql`) +
  `EditorPrefRepo` + `GET`/`PUT /editor-prefs` persist per-item editor
  heights server-side. Note editor heights are applied and saved.

### Fixed — Note focus scroll lands on long node bodies
Extended the `focus_note` retry schedule (added 3000 ms / 5000 ms steps)
so the deep-link scroll still reaches a note after a long node body
finishes rendering.

## [2.15.1] - 2026-05-30

### Changed — Markdown rendering in task titles
Task titles now render inline Markdown (bold, italic, strikethrough,
inline code, links) wherever they are displayed — task row, task panel,
Inbox, and calendar — via a new `render_markdown_inline` helper. Block
syntax is flattened to a single line so truncated rows keep their layout.
Notes were already block-rendered everywhere.

## [2.15.0] - 2026-05-30

### Added — Note click-through focus and delete
Clicking a node-attached note in the feed now deep-links to
`/nodes/:id?note=<id>`; the NotePanel auto-expands and scrolls to / flashes
the target note (cards carry a `data-note-id`), mirroring task
click-through. Notes can be deleted from the feed with an inline
confirm via owner-checked `NoteRepo::delete` + `DELETE /notes/:id`.

## [2.14.0] - 2026-05-30

### Added — Notes feed filter & sort toolbar
The Notes feed gains a toolbar: sort (newest / oldest / recently updated),
node filter (All / Uncategorized / a specific node), a date range, a
debounced text search, and Reset. Server-side, `feed_for_owner` and
`feed_all` collapse into a single `feed(owner, NoteFeedFilter)` using
guarded SQL (`$n IS NULL OR …`) and an enum-driven `ORDER BY`.

## [2.13.0] - 2026-05-30

### Added — Standalone (inbox) notes
Notes can now be posted without a parent node, like inbox tasks.
`node_notes.node_id` is nullable (migration `028_note_node_id_nullable`);
`Note.node_id` / `FeedNote.node_title` became `Option`; the feed query
switched `JOIN` → `LEFT JOIN`; `POST /notes` accepts a `node_id` in the
body or none at all. The Notes view gains a compose box, and node-less
notes render an "Inbox" pill.

## [2.12.1] - 2026-05-30

### Security — Dropped the legacy AWS-SDK rustls 0.21 chain
Set `default-features = false` on `aws-sdk-{s3,cognitoidentityprovider,sesv2}`
and dropped their legacy `rustls` feature (which pulled
`aws-smithy-runtime/tls-rustls` → hyper-rustls 0.24 / rustls 0.21 /
rustls-webpki 0.101.7), keeping the modern `default-https-client`
(rustls 0.23 + aws-lc-rs, already in tree via reqwest). Removed the
RUSTSEC-2026-0049 / 0098 / 0099 / 0104 ignores — the affected package is
gone and `cargo audit` exits 0 (548 deps, down from 556). Verified with a
live S3 TLS round-trip (fake credentials → parsed `403 InvalidAccessKeyId`
= a real handshake on the aws-lc-rs connector).

## [2.12.0] - 2026-05-29

### Added — Per-request tracing & metrics
`tower-http`'s `TraceLayer` emits a per-request INFO span
(method / uri / request-id / latency) and a `RequestMetrics` set of atomic
counters bucketed by status class, surfaced in the admin `/api/metrics`
endpoint under a new `requests` field. No new dependencies.

### Changed — PKCE verifiers moved to Postgres
PKCE verifiers moved from an in-memory `Arc<Mutex<HashMap>>` to Postgres
(migration `027_pkce_verifiers.sql`, new `PkceRepo`, `AppState.pkce`).
In-flight logins now survive an API restart or redeploy, which previously
produced `invalid_code_verifier` at the callback. `take` consumes-once
(`DELETE … RETURNING`) and a sweeper clears expired rows.

### Security — UI container runs as non-root
The UI image is now `nginxinc/nginx-unprivileged:alpine`, listening on
8080; the proxy, compose, and k8s port references were updated to match,
and `Permissions-Policy` was added to the UI image's own nginx config.
Closes the deferred Docker-hardening gap (S3-4).

### Changed — Consolidated cargo-audit configuration
The two divergent cargo-audit ignore lists collapse into a single
`.cargo/audit.toml` (one source of truth, dated rationale); CI runs a bare
`cargo audit`.

## [2.11.2] - 2026-05-29

### Changed — Shared `IconButton` component for inline actions
Inline-edit Save/Cancel controls had drifted across panels: the Kanban
task row and Inbox used amber text-label buttons, while other panels used
hand-rolled icon-only buttons with copy-pasted Tailwind hover classes.
Extracted a single `IconButton` component (`ui/src/components/icon_button.rs`)
with tooltip, accessible label, hover-color variants
(Neutral / Save / Danger / Accent), an optional disabled signal, and a
`stop_propagation` flag for buttons inside clickable rows. Converted the
text-label Save/Cancel in `task_row` (Kanban) and `inbox_view` to icons,
and deduplicated the existing icon-only inline buttons in `links_panel`,
`note_panel`, `tag_manager`, `task_panel`, and `templates_view` onto the
shared component. Modal-footer and full-form CTAs intentionally keep their
text labels. No API or behaviour changes.

## [2.11.1] - 2026-05-29

### Changed — Pinned Rust toolchain bumped 1.92 → 1.96
The pinned toolchain in `rust-toolchain.toml` lagged the locally-installed
and CI/Docker stable (all 1.96), which produced recurring `E0514`
"incompatible version of rustc" conflicts in the shared `target/` dir
whenever a non-rustup cargo touched the build. Bumped the pin to `1.96`
to align every build surface on one compiler. Dropped the now-false
"pinned to rustc 1.92" rationale on the CI `sqlx-cli` install (kept the
`--version 0.8.6` pin). No application behaviour changes.

### Fixed — Two new clippy 1.96 lints
- `unnecessary_sort_by` in `node_list.rs` — node "modified" sorts now use
  `sort_by_key` (`std::cmp::Reverse` for descending).
- `manual_checked_ops` in `project_dashboard.rs` — the project completion
  percentage uses `checked_div` + `map_or` instead of a manual `if > 0` guard.

## [2.11.0] - 2026-05-28

### Added — Harmonized add-task forms (shared `NewTaskForm`)
The in-node add-task UI and the Inbox add-task UI had drifted apart —
different controls, different layout, different validation feedback.
Extracted the in-node form into a reusable `NewTaskForm` component
(icon `+` button, priority / due / recurrence chips, inline
validation error) and wired both `TaskPanel` and Inbox to it so the
two entry points are visually and behaviourally identical. Inbox
additionally gains an **optional node picker** so a captured task can
be linked to a node at creation time.

### Fixed — My Day tasks now stick until removed or completed
A task added to My Day (its `focus_date` set via the lightbulb)
silently dropped out of the **Today** zone at midnight: the read
filter matched `focus_date == today`, but the lightbulb writes a
literal date, so the next day the task no longer matched. Carry-forward
tasks (a past-but-open `focus_date`) must stay in Today until the user
manually removes them (clears `focus_date`) or completes them.

The Today-zone read boundary is now `focus_date <= today` and the
Backlog boundary is `focus_date` none-or-future, mirroring the server
`list_my_day` carry-forward predicate. The "carried from" badge now
renders in the Today zone as well, so carried items are clearly
labelled.

---

## [2.10.1] - 2026-04-28

### Fixed — Sidebar header truncated "Ember Trove" after the (?) icon landed
The new help-toggle button added in v2.10.0 plus the existing
dark-mode toggle plus an inline version pill all competed for the
same horizontal slot in the sidebar header.  At the default
`md:w-64` width (16rem) the title got truncated to "Ember Tro…".

Stacked the version under the title (`flex flex-col` on the inner
container instead of `flex items-baseline`).  Title now owns its own
line and the version reads as a footnote underneath, matching the
visual weight it actually deserves.  No width change to the sidebar,
so main-content viewport stays the same.

---

## [2.10.0] - 2026-04-28

### Added — In-app help (Shortcuts / Concepts / Workflow)
The Apr-2026 UX pass added a real conceptual model on top of the
schema (focus_date is binary, PARA grouping via `contains` edges,
Carryover automatic, etc).  None of that was discoverable from the
UI.  This release surfaces it.

- **`ShortcutsModal` is now `HelpModal`** — same `?` shortcut, same
  modal pattern, but tabbed: **Shortcuts** (existing keyboard
  reference, unchanged), **Concepts** (10 short definitions of the
  data model — Nodes, Tasks, Notes, focus_date vs due_date, PARA,
  Inbox, Carryover, Pinning, Tags vs Areas, Recent + Search), and
  **Workflow** (7 numbered steps for the day-to-day loop, capture
  → triage → focus → end-of-day → weekly review).
- **`(?)` icon in the sidebar header** — small `help_outline` button
  next to the dark-mode toggle.  Discoverable for users who don't
  know the `?` shortcut yet; both paths flip the same signal via the
  new `ShowHelp` context.
- **Version stamp** in the modal footer — auto-fills from
  `CARGO_PKG_VERSION` at compile time so it can never drift behind a
  release.

### Implementation notes
- `ui/src/components/modals/shortcuts.rs` deleted; replaced by
  `ui/src/components/modals/help.rs`.  Content tables are static
  (no localStorage, no API) — written deliberately as headings +
  short bullets so updates are surgical.
- `ShowHelp(RwSignal<bool>)` newtype provided at the layout root;
  `SidebarHeader` reads it as an `Option<ShowHelp>` (no crash if
  someone uses the sidebar outside the layout).  Same signal that
  the `?` keyboard handler already toggles, so mouse and keyboard
  share one source of truth.

### Discipline note (carried into the release checklist)
When a future release changes a user-visible model or workflow,
update the corresponding tab in `help.rs`.  Stale help is worse
than no help — but the static-content + version-stamp design
makes the rot visible (the footer says which version the help
matches; if Concepts mention a feature that no longer exists, it's
on the next release to fix).

---

## [2.9.0] - 2026-04-28

### Added — Dashboard PARA + pinning + activity recap (UX phase 7)
The Project Dashboard is now organised the way a BASB / PARA user
actually thinks about projects: grouped under their parent **Area**,
with the most-active and **pinned** projects floated to the top of
each group, and a *what changed in the last 48 hours* recap at the
top of the page so you don't have to drill into each project to see
what moved.  This is the seventh and final phase of the Apr-2026 UX
pass.

- **PARA grouping.**  `ProjectDashboardEntry` gains `area_id` and
  `area_title` fields, populated server-side from the `edges` table
  via a new `NodeRepo::area_for_nodes` query.  An Area "owns" a
  Project via an `edge_type = 'contains'` edge from the Area to the
  Project; oldest such edge wins when a project belongs to multiple
  Areas (`ROW_NUMBER() OVER (PARTITION BY project ORDER BY
  created_at)`).  Projects with no Area parent land in an
  **Ungrouped** bucket at the bottom of the list.
- **Pinning.**  `ProjectDashboardEntry.pinned` is now populated
  from `nodes.pinned`.  New `NodeRepo::set_pinned` + handler at
  `PUT /api/nodes/:id/pin` body `{"pinned": bool}` (placeholder
  route test from earlier finally has a real handler under it).
  Click the ★ next to a project's title to pin / unpin; pinned
  projects sort to the top of their Area, then by `last_activity_at`
  desc.  Optimistic UI with toast + rollback on failure.
- **Activity recap section.**  New
  `ActivityRepo::list_recent_for_owner` JOINs `activity_log` to
  `nodes` so the user's recent activity surfaces in one round-trip
  with parent-node titles.  New endpoint
  `GET /api/dashboard/activity?since=<rfc3339>&limit=<n>` (defaults
  48h / 50 rows).  The dashboard renders this above the project
  groups, split into "Today" and "Yesterday" sub-headings; hidden
  entirely when there's nothing to recap so empty cases don't
  generate visual noise.
- **`RecentActivityEntry`** DTO added to `common::activity`
  (`#[serde(flatten)]`'s an `ActivityEntry` plus a `node_title`).

### Implementation notes
- Dashboard fetch is now two LocalResources (projects + activity)
  driven by a single shared `dashboard_refresh` signal, so a pin
  toggle re-fetches both and the optimistic ★ state lines up with
  the re-sorted project order.
- Sort within each Area: pinned DESC then `last_activity_at` DESC.
  Pinned-vs-pinned and unpinned-vs-unpinned both honour recency, so
  pinning isn't a hard freeze.
- Stub repos (`StubNodeRepo`, `StubActivityRepo`) updated for the
  new trait methods — required by CLAUDE.md per-trait-method
  invariant.

### Tests
- `dashboard_activity_route_registered` — route registration
  regression test.
- `node_pin_route_registered` (pre-existing placeholder) — was
  vacuously passing on the OIDC=None test path; now backed by a
  real handler.
- API total: 61 (was 60); common total: 40 (unchanged).

### Out of scope (deferred / intentionally not in v2.9.0)
- Multi-Area projects: currently we surface the oldest containing
  Area only; rendering one project under multiple Areas would
  duplicate cards on the dashboard, which we judge worse than
  picking a stable parent.  If a real use case appears, expose a
  "primary area" field separately.
- Per-project sort customisation: only pinned-vs-recent for now.
- Activity recap on mobile is a single column; no pagination yet
  even though `since`/`limit` are query-tunable.

---

## [2.8.0] - 2026-04-28

### Added — Cmd-K command palette (UX phase 6)
A floating overlay over the current view that lets you jump to any
node, search nodes, or create a new one without breaking out of where
you are.  Replaces `/`'s old behaviour of navigating to the full-page
`SearchView` (still reachable by URL).

- **Hotkeys to open:**
  - `⌘K` / `Ctrl-K` — works anywhere, even mid-edit (one of the few
    shortcuts that intentionally bypasses the input-focus guard, since
    it's a system-wide affordance the user expects to always work).
  - `/` — repurposed from full-page navigation to opening the palette.
- **Sections:**
  - **Recent** (top 5 from `crate::recent::read_recent`) when query is
    blank — instant fast path, no typing.
  - **Matches** (live, 300ms-debounced) when query is non-empty.  Calls
    the existing `node_picker_search` (returns up to 8 results).  Stale
    responses dropped via the canonical version-counter pattern from
    `.claude/patterns/reactive-effect-debounce.rs`.
  - **Create node titled '<query>'** as the bottom action whenever the
    query is non-empty.  Always present even when there's an exact
    match — sometimes you want to create another node with the same
    title.
- **Keyboard model inside the palette:**
  - `↑` / `↓` move the highlight; `Enter` picks; `Esc` closes.
  - Typing resets the highlight to the first item so `Enter` always
    lands somewhere sensible.
  - Backdrop click also closes.
- **"Create node" picks** open the structured `CreateNodeModal`
  pre-filled with the typed title (new `initial_title` prop on
  `CreateNodeModal` alongside the existing `initial_body` for
  fast-capture handoff).

### Implementation notes
- New file: [`ui/src/components/modals/command_palette.rs`](ui/src/components/modals/command_palette.rs)
  with a typed `PaletteAction` enum (OpenNode / CreateNode) so the
  Enter dispatch is exhaustive — no string-keyed payload.
- The palette's open state lives in `layout.rs` as a single
  `RwSignal<bool>`; the `⌘K` listener and the `/` shortcut both flip
  it.  No global state pollution, no app-wide context.
- The two `window_event_listener` registrations in `layout.rs` are
  intentionally separate: the `/`-handler short-circuits on any
  modifier (correct for `n`, `g`, `?`), so `⌘K` needs its own
  listener that explicitly only fires on the modifier path.
- `ShortcutsModal` updated — `/` now reads "Open command palette",
  `⌘K` added as the alternate shortcut.

### Out of scope (deferred)
- Filter chips (type / tag) inside the palette — the existing
  `SearchView` still owns advanced filtering.  The palette is
  optimised for "I know roughly what I'm looking for, just get me
  there fast."
- Search-result snippets inside the palette — list shows title +
  type icon only.  `SearchView` is still the place for snippet
  preview + sort + pagination.
- Mobile gesture for opening (no on-screen keyboard equivalent of
  `⌘K`).  PWA users who installed the home-screen icon get the same
  three shortcuts the manifest declared in v2.4.0 (Quick capture, My
  Day, Inbox); the palette is desktop-first.

---

## [2.7.0] - 2026-04-28

### Added — Kanban keyboard triage (UX phase 5)
The My Day Kanban now drives from the keyboard.  Inbox-zero / triage
loops that previously needed mouse clicks per row become a single
`j j j t Space d` rhythm.  Sits cleanly on top of the v2.6.x
`KanbanTaskRow` foundation — no row layout changes, just new context
plumbing for focus + edit cursors.

**Shortcuts** (active on `/tasks/my-day`, suppressed when an input,
textarea, select, button, or contenteditable has focus):

| Key            | Action                                                       |
|----------------|--------------------------------------------------------------|
| `j` / `↓`      | Focus next task (across both zones, in display order)        |
| `k` / `↑`      | Focus previous task                                          |
| `Enter`        | Open the focused task in its parent (or `/tasks/inbox`)      |
| `Space`        | Toggle done on the focused task                              |
| `t`            | Toggle the focused task between Today and Backlog            |
| `e`            | Open inline edit on the focused task                         |
| `d`            | Delete the focused task                                      |

`s` (snooze) from the original Phase 5 spec is intentionally absent.
With `focus_date` binary (today | None) under the v2.6.0 model,
"snooze" is the same gesture as "remove from today" — already covered
by `t` from the Today zone.

### Implementation notes
- New context types in [`task_row.rs`](ui/src/components/task_row.rs):
  `FocusedTaskId(RwSignal<Option<TaskId>>)` and
  `EditingTaskId(RwSignal<Option<TaskId>>)`.  `MyDayView` provides
  both at the top of the view; `KanbanTaskRow` reads them to render a
  focus ring (inset amber `box-shadow`) and to drive its inline edit
  form.  Mouse and keyboard share one focus cursor — clicking a row
  also moves the keyboard cursor there.
- The window keydown handler lives in `MyDayView` so it auto-detaches
  when the user navigates elsewhere (Leptos drops the
  `window_event_listener` handle on view unmount).  Modifier keys
  (Ctrl / Cmd / Alt) are reserved for app-level shortcuts (e.g. the
  forthcoming Cmd-K palette) and never consumed.
- Pencil-button click and `e` shortcut now share one mechanism:
  both write to the `EditingTaskId` context signal, so opening edit
  from either path is identical.
- `j` / `k` after navigation scroll the focused row into view via
  `scrollIntoView({block: "nearest"})` — quiet, no flash.
- `ShortcutsModal` (`?`) reorganised into three groups: **Anywhere**,
  **My Day Kanban**, **Node view** — mirrors where the shortcuts are
  actually active.

### Out of scope (not regressed by this release)
- Capture-from-anywhere (`n`) and search (`/`) shortcuts unchanged —
  still listed under "Anywhere" in the help overlay.
- The Cmd-K palette (Phase 6 / v2.8.0) will repurpose `/` to open a
  floating palette over the current view; until then `/` still
  navigates to the full-page `SearchView`.

---

## [2.6.2] - 2026-04-28

### Added — Click-to-navigate + inline edit on Kanban tasks
Real-world feedback after a few hours with v2.6.0/v2.6.1: the Kanban
rows had no way to open the parent node, no way to edit a task's
title/due-date/priority/recurrence inline.  Both fixed without
sacrificing drag-to-swap-zones.

- **Row click navigates** to the task's origin:
  - Task with parent → `/nodes/{node_id}?task={task_id}`
  - Standalone (Inbox) task → `/tasks/inbox?task={task_id}`
  - The destination view (`NodeView` / `InboxView` / `TaskPanel`)
    scrolls the matching `[data-task-id="<id>"]` row into view and
    briefly flashes it amber via the new `focus-task-flash` CSS
    keyframe.  Driven by `crate::focus_task::schedule_focus_task`,
    which retries a few times to handle the LocalResource-load race.
    The `?task=` param is `replaceState`'d out so a refresh doesn't
    re-fire the highlight.
- **Pencil button** added between the zone-swap button (☀ / ×) and
  the delete button.  Opens an inline edit form with title, priority
  (low/medium/high chips), due date, and recurrence (daily / weekly /
  biweekly / monthly / yearly).  Save persists via PATCH; Esc cancels.
  No `focus_date` field — focus is binary, owned by the zone-swap
  button.
- **Action-button click handlers all `stopPropagation()`** so they
  never trigger the row-click navigation.  Drag still works because
  HTML5 dragstart fires only on mousedown + movement; click fires on
  mousedown + mouseup without movement.

### Fixed — Hover-flicker on rapid mouse-over
The row's `class=move ||` re-evaluated on every status change,
swapping class strings and occasionally leaving a transient border
artifact when the cursor swept the list quickly.  Class string is
now static; the only mutable bit (opacity for done tasks) lives in a
clean `style=move ||` attribute that Leptos diffs without surprises.

### Changed — Parent-node chip is now amber
The project name above each task title was the same colour as the
title meta and easy to miss.  Now `text-amber-700` (light) /
`text-amber-400` (dark) with semibold uppercase tracking, matching
the app's primary accent (Today-zone left border, priority dots,
focus-task flash, ☀ icons).  The icon (`rocket_launch` for project
tasks, `inbox` for standalone) gets the same amber tint.

### Behavioural clarification (no code change, captured in CHANGELOG
### so the model is unambiguous)
A task in the Today zone that you don't complete by end of day does
**not** drop into limbo.  Its `focus_date` stays at the date you
last set it.  Tomorrow, "today" advances by one day, the partition
flips it into the **Backlog** zone, and `KanbanTaskRow` adds a
"carried from Apr 28" amber badge so you can see it slipped.
`TaskRepo::list_open_for_owner` returns every open task regardless
of `focus_date`, so the task is always visible somewhere until it's
completed (status=Done) or cancelled — at which point it's done, and
correctly disappears from the backlog query.

---

## [2.6.1] - 2026-04-28

### Removed — `/plan` route and morning-ritual surface
The Kanban shipped in v2.6.0 made `/plan` redundant.  Side-by-side
review with the user:

| `/plan` section | Where it lives now | Verdict |
|---|---|---|
| Carry-over count | Kanban backlog rows show "carried from X" badge | Redundant |
| Inbox count | Kanban backlog includes Inbox-chipped rows | Redundant |
| Due today | Kanban sorts due-first; deadlines float to top | Redundant |
| Yesterday recap | Nowhere | User opted not to keep it |
| "Start my day" CTA + `et.plan.last_planned_at` stamp | Drove the banner on `/tasks/my-day` | Banner removed too |

Net deletion:

- `ui/src/components/plan_view.rs` (entire file, plus its `pub mod`
  in `components/mod.rs`).
- `<Route path=path!("/plan") view=PlanView />` and the import in
  `layout.rs`.
- "Plan" sidebar link in `sidebar.rs`.
- "Plan your day — review yesterday and inbox" banner block in
  `my_day_view.rs` (and its `planned_today()` import).
- `LAST_PLANNED_AT_KEY` localStorage key — no code reads or writes
  it anymore; existing stored values become inert and harmless.

The Kanban's per-zone empty states (introduced in v2.6.0 — "Nothing
on today's list — drag or tap ☀ on a backlog task below" /
"Your backlog is empty.") already replaced what the v2.5.0 cold-start
copy used to point at, so nothing else needed touching.

### Roadmap impact
The Apr-2026 UX phase plan now spans phases 1–4 + this trim, with
Phase 5 (Inbox keyboard triage on the shared `KanbanTaskRow`) on
deck for v2.7.0.  No other phases changed.

---

## [2.6.0] - 2026-04-28

### Changed — My Day is now a two-zone vertical Kanban (UX phase 4)
Direct response to user feedback that the planning workflow was unclear:
"how do I push today's tasks to another day?" and "how do I pull an
old task into today?" had no good answers in v2.5.x.  Replaces the
group-by-project layout with a Kanban so both questions become "drag
between zones, or tap the button."

- **Top zone — Today.** Tasks with `focus_date == today`.  The "what
  I committed to do today" surface.
- **Bottom zone — Backlog.** Every other open task across every node,
  sorted by `due_date ASC NULLS LAST`, then priority desc, then
  `created_at ASC`.  Powered by a new server query
  `TaskRepo::list_open_for_owner` exposed at `GET /api/tasks/all`.
- **Two equivalent ways to swap a task between zones:**
  - Tap the ☀ "Add to today" button (in backlog) or × "Remove from
    today" button (in today) on any row.  Always visible — never
    hover-to-reveal — so the touch path matches the desktop path.
  - Drag the row from one zone to the other.  HTML5 native drag and
    drop; the destination zone fires the same `PATCH /api/tasks/:id`
    the tap button would.  Touch never fires `dragstart` so iPhone
    users simply tap.

### Simplified mental model
- **`focus_date` is binary.** Only `Some(today)` or `None`.  No more
  "schedule for next Tuesday" affordance on My Day rows.  The task
  editor still lets you change `due_date` (the external deadline);
  `focus_date` is purely the Kanban zone.  This is a deliberate
  simplification after the user said "the date the task should be
  worked can be simply 'today' or 'not today'."
- **Carry-over is no longer a separate concept on My Day.**
  Carryovers (open tasks whose `focus_date` is in the past) just
  appear in the backlog with a small "carried from May 2" badge for
  context.  The badge tells you the task has been sitting; the
  ☀ tap (or drag to today) brings it back.
- **`/plan` Carry Over section becomes a count.** "N tasks carried
  over from earlier days → /tasks/my-day".  The triage UI lives in
  one place — the Kanban — and the morning ritual just nudges the
  user toward it.

### Added — shared `KanbanTaskRow` component
Lives in `ui/src/components/task_row.rs`.  Drives both Kanban zones
via a `KanbanZone::Today | Backlog` enum that swaps the zone-swap
button.  Foundation for v2.7.0 (keyboard triage `j/k/m/c/d/e`) which
will plug in here without touching layout code.

### Removed
- Dedicated **Carry Over section** in My Day (logic merged into the
  backlog zone with a "carried from X" badge).
- The reschedule date popover that briefly lived on carryover rows in
  v2.4.1 / v2.5.0 — under the binary `focus_date` model there's
  nothing to "reschedule to a specific day", and the editor still
  handles `due_date` mutations.
- `ui/src/components/carryover.rs` (deleted; its CarryoverSection
  was used only by the now-deleted carry-over surface in MyDayView
  and by the simplified count on `/plan`).

### Backend
- **New trait method** `TaskRepo::list_open_for_owner(owner_id) ->
  Vec<MyDayTask>` and `StubTaskRepo` impl in `api/src/tests.rs`.
- **New route** `GET /api/tasks/all` (auth required) returning
  every open task for the caller, joined with parent node title.
  Sort: due_date ASC NULLS LAST, then priority desc (high→low),
  then created_at ASC.  No new schema, no migration.
- **Route-registration regression test** `tasks_all_open_route_registered`.

### Out of scope (deferred)
- Filter / sort affordances on the backlog (project filter, "high
  priority only" toggle).  Default sort is good enough for v2.6.0;
  filters arrive when the backlog gets large enough to need them.
- Inline task edit on Kanban rows.  Edit currently means: navigate to
  the task's parent node and edit there.  Inline edit returns in a
  later phase — keyboard triage (Phase 5) will need it.
- Shared TaskRow for `/tasks/inbox` and `task_panel` — InboxView and
  TaskPanel still use their existing row implementations.  Migrating
  them is a follow-up cleanup since v2.6.0 already validates the
  shared row in two zones.

---

## [2.5.1] - 2026-04-28

### Fixed — Real-world feedback on the v2.5.0 planning ritual
Surfaced after a day's use.  Three bugs and a labeling fix; no schema
or API surface change.

- **`/plan` "items to triage" no longer counts done/cancelled tasks.**
  `TaskRepo::list_inbox` returns *all* standalone tasks regardless of
  status, so the Inbox section was showing "3 items to triage" even
  when all three were already completed.  Filter applied client-side
  via `task_common::status_done` so the count reflects open work only.
  Confirmed in browser before/after — a user with 0 open + 3 done
  inbox tasks now correctly sees "Inbox is empty".  Server-side fix
  deferred to v2.6.0 since it would require a coordinated change with
  `InboxView` (which already partitions correctly on the client).
- **Carry-over rows now show the parent node name as a visible chip.**
  Previously the parent label rendered in a small grey meta line that
  got `truncate`d to nothing on narrow viewports and was invisible
  whenever the action buttons consumed row width.  Lifted to its own
  full-width meta row above the title with a `rocket_launch` /
  `inbox` icon mirroring the iconography used in `MyDayGroup`, so
  context survives at any width.
- **My Day clarifying subhead.** The page just said "My Day" with a
  date.  When a user triaged carryovers via the "Today" button (which
  sets `focus_date = today`) they saw tasks with future `due_date`s
  show up and were confused.  Subhead now reads "tasks you're focused
  on today (focus date = today; due date is separate)".  Honest and
  short.
- **"due" prefix on date labels in MyDayTaskRow.** A row showing "May 6"
  in the corner is ambiguous — looks like a focus date, looks like a
  deadline, looks like nothing in particular.  Now reads "due May 6"
  (or "⚠ due May 6" when overdue) with a "External deadline" tooltip,
  so the date's meaning is unambiguous at a glance.

### Out of scope for this point release
The refactor work surfaced by the user's "can we reuse code?" question
— extracting a shared `TaskRow` used by `InboxView`, `MyDayTaskRow`,
`task_panel`, `CarryoverRow`, and `plan_view::CalRow` — and the new
**Backlog tab** in `TasksView` showing all open tasks across nodes
both move into v2.6.0 (which displaces the original "Inbox keyboard
triage" plan to v2.7.0; keyboard triage will sit cleanly on top of
the unified `TaskRow`).

---

## [2.5.0] - 2026-04-27

### Added — Morning Planning Ritual at `/plan` (UX phase 3)
A once-per-day surface that turns "look at My Day" into "plan your
day."  Inspired by Sunsama's daily ritual but stripped down — no
dragging, no time-blocking, no AI scheduling.  Just the four things
you actually need to decide before the day starts.

- **`/plan` route** ([`ui/src/components/plan_view.rs`](ui/src/components/plan_view.rs))
  with four sections:
  1. **Yesterday** — done / open / cancelled counts for tasks whose
     `focus_date` was yesterday.  Derived client-side from
     `fetch_my_day(yesterday)` filtered to `focus_date == yesterday`
     so day-2 carryovers aren't double-counted with the carry-over
     section.
  2. **Carry over** — reuses the v2.4.1 `CarryoverSection` so the
     Today / Reschedule / Drop actions match the My Day surface.
  3. **Inbox** — count + jump-to button.
  4. **Due today** — read-only peek at tasks with `due_date == today`,
     pulled from the existing month-window calendar fetch.
- **"Start my day" CTA** stamps `et.plan.last_planned_at` in
  localStorage with today's date, then navigates to `/tasks/my-day`.
- **My Day plan-your-day banner** appears at the top of `/tasks/my-day`
  whenever `et.plan.last_planned_at != today`, so the ritual is
  discoverable from the user's normal entry point — no need to know
  the URL.  Dismisses itself once the user confirms today.
- **My Day cold-start empty state** rewritten to surface both the `n`
  shortcut and a "Plan your day" CTA, instead of assuming the user
  already has a project to attach tasks to.
- **Sidebar entry "Plan"** with a `wb_twilight` icon, just above
  Tasks, so the ritual is one click from anywhere.
- **`CarryoverSection` extracted** from `my_day_view.rs` into its own
  `crate::components::carryover` module so both `/plan` and
  `/tasks/my-day` import the same component.

### Design notes (decision log)
- **No new server endpoints.**  Yesterday's stats, carryover, inbox,
  and today's calendar are all derived from the existing
  `fetch_my_day` / `list_inbox` / `fetch_calendar_tasks` endpoints.
  Keeps the phase small and avoids a schema/API surface change for a
  pure UX layer.
- **`/plan` over a banner-only surface.**  A dedicated route is
  bookmarkable, can be set as the PWA home-screen shortcut later, and
  doesn't compete with the carry-over section for vertical space on
  My Day.
- **`et.plan.last_planned_at` is per-device.**  localStorage means
  different devices need to be planned independently.  This matches
  how the user actually works (morning planning happens on the laptop
  they have at hand) — a server-side stamp would be over-engineered.

### Out of scope (left for later)
- Time-blocking / hour-budget warnings (Sunsama-style).
- Goal/objective alignment, OKR rollups.
- Auto-redirect of the PWA `start_url` to `/plan` until planned —
  considered for Phase 6 if the banner alone isn't enough nudge.

---

## [2.4.1] - 2026-04-27

### Added — Carry-over section in My Day (UX phase 2)
The server already carried unfinished tasks forward (`focus_date < today`
and not done both surface in `list_my_day`), but the UI wove them
silently into today's groups with only a tiny "carried over" badge.
Easy to miss, no triage path. Today's "Did I plan this?" question
required reading every row.

- Tasks with `focus_date < today` now render in a dedicated **Carry
  Over (N)** section pinned above today's groups
  ([`my_day_view.rs`](ui/src/components/my_day_view.rs) — partition step
  in the main render closure plus new `CarryoverSection` and
  `CarryoverRow` components).
- Three single-tap actions per row:
  - **Today** — `focus_date = today`, stays in My Day.
  - **Reschedule** — toggles a small date input; the picked date becomes
    the new `focus_date`.
  - **× (Drop)** — clears `focus_date`; the task drops back to the
    Inbox (or stands alone if it was already orphaned).
- Section is hidden when there are no carryovers — first-time users
  never see an empty "Carry Over (0)" header.
- All actions go through the existing `PATCH /tasks/:id` endpoint via
  `UpdateTaskRequest::focus_date: Some(Some|None)`; no schema change.
- Toast on each action so the result is visible without scanning the
  newly re-rendered list.

This phase deliberately stays small — it surfaces the existing
carryover signal without adding triage rituals or batch ops. Phase 3
(morning-planning ritual) will reuse this `CarryoverSection` so the
component pulls double-duty.

---

## [2.4.0] - 2026-04-27

### Added — iOS Quick Capture (UX phase 1)
First piece of a six-phase second-brain/GTD usability pass.  Closes the
single biggest mobile friction point: there was no way to land a thought
in Ember Trove from an iPhone without unlocking, opening the PWA,
navigating, and pressing `n` (which doesn't exist on iOS soft
keyboards).

- **`POST /api/inbox/quick`** — auth-required endpoint that takes
  `{title, body}`, coalesces them into one Task title (max 500 chars,
  Unicode-safe truncation, control chars stripped), creates the task
  with `node_id IS NULL` so it lands in the existing tasks-Inbox view.
  See [`common::inbox::coalesce_capture`](common/src/inbox.rs:62) for
  the rules and round-trip tests.
- **PWA Web Share Target** — `manifest.json` declares
  `share_target.action = "/share"`.  The service worker (`ui/public/sw.js`,
  cache bumped to `ember-trove-v5`) intercepts that POST, forwards the
  multipart fields to `/api/inbox/quick`, and 303s to
  `/tasks/inbox?captured=1`.  Result: Trove appears in the iOS / Android
  Share Sheet for any app's text or URL.
- **PWA shortcuts** — manifest declares three home-screen long-press
  shortcuts (Quick capture, My Day, Inbox).
- **`FastCaptureModal`** (`ui/src/components/modals/fast_capture.rs`) —
  one autofocused textarea, Cmd/Ctrl+Enter saves, Esc closes,
  "More fields…" hands off to the structured `CreateNodeModal` with
  the draft pre-filled (no lost typing).  The `n` shortcut now opens
  this instead of the structured modal — friction floor restored.
- **InboxView toast on capture** — reads the `?captured=1` marker, fires
  a success toast, and `replaceState`s the URL clean so a refresh
  doesn't re-fire it.

### Decision log
- Capture target is a `Task` (with `node_id IS NULL`), not a `Node`.
  Tasks already drive the Inbox triage flow the user does daily, and
  Notes require a parent node so couldn't be the inbox surface.  An
  `Inbox` `NodeType` was considered and rejected — would have needed a
  migration plus duplicate sidebar/filter wiring for no behavioural win.
- The structured `CreateNodeModal`'s "default type from active filter"
  behaviour was kept.  The friction it caused only existed because `n`
  used to open it for ad-hoc dumps; now `n` opens fast-capture, and
  filter-aware defaults are correct when the structured modal is
  reached deliberately (e.g. from a future "+" on a filtered All Nodes
  view).

### Follow-ups
- Apple Shortcut export for Siri "Capture to Trove" — deferred; the
  Web Share Target reaches ~95% of the win without distribution.
- A `description: Option<String>` field on `Task` so >500-char captures
  don't truncate.  Not blocking — share-sheet captures from Safari /
  Mail / Messages are well under that limit in practice.

---

## [2.3.9] - 2026-04-27

### Fixed — Auth callback no longer renders raw JSON 500 to the browser
- **`/api/auth/callback` now redirects on every failure mode instead of
  returning a JSON `ApiError`.**  Cognito redirects the browser directly
  to the callback URL, so a 500 + `{"error":"internal error"}` body was
  rendered by the browser as the literal page contents — the user saw a
  wall of JSON whenever the OAuth handshake failed.  The handler now
  wraps its work in an inner `try_callback` and converts any `ApiError`
  into a 303 redirect to `frontend_url`, where the SPA's `AuthGate`
  starts a fresh login flow.  A missing PKCE verifier (entry evicted
  by the 10-min TTL or wiped by a container restart) and a missing
  OAuth `state` query param both short-circuit cleanly to the same
  redirect path.
- **`OidcClient::exchange_code` reclassifies Cognito 4xx as
  `Unauthorized`, not `Internal`.**  An `invalid_code_verifier` /
  `invalid_grant` from Cognito is an auth-flow failure caused by stale
  browser state, not a server bug.  This stops the case from
  generating ERROR-level log noise and aligns it with the existing
  treatment in `exchange_refresh_token`.
- **Regression coverage** — added `auth_callback_redirects_on_misconfig_instead_of_json_500`
  and `auth_callback_redirects_when_state_param_missing` in
  `api/src/tests.rs`, asserting `303 See Other` + `Location:
  http://localhost:3000` rather than the previous JSON 500.

### Known follow-up (not in this release)
- The PKCE verifier is still kept in an in-memory
  `Mutex<HashMap<String,(String,Instant)>>` on `AppState`, so every
  container restart wipes in-flight OAuth flows and mid-login users
  bounce back to the login screen.  Moving the verifier to a
  short-lived encrypted cookie (the `PrivateCookieJar` is already in
  scope on `login` / `callback`) would eliminate both the restart
  volatility and the 10-minute TTL race entirely.  Tracked as a
  separate task.

---

## [2.3.8] - 2026-04-26

### Security / CI hygiene
- **Bumped `rustls-webpki` 0.103.12 → 0.103.13** to resolve
  RUSTSEC-2026-0104 ("Reachable panic in certificate revocation list
  parsing").  The bump applies cleanly to the modern path
  (reqwest + jsonwebtoken).  The legacy 0.101.7 path pulled in by
  `aws-smithy-http-client → hyper-rustls 0.24 → rustls 0.21` has no
  upstream fix; we don't feed CRLs to the AWS SDK rustls config, so the
  panic is unreachable in practice.  Added
  `--ignore RUSTSEC-2026-0104` to the cargo-audit step in `ci.yml`
  with a dated rationale alongside the existing 0098 / 0099 ignores
  (same legacy path).  Drop once `aws-smithy-http-client` bumps to
  `rustls 0.23+`.

---

## [2.3.7] - 2026-04-26

### Fixed — Mobile UI
- **Project Dashboard: project title now visible on portrait phones** —
  the dashboard card top row packed title + status + 4 count badges +
  progress bar onto a single horizontal flex row, which crushed the
  title to zero width on narrow viewports and left users with no way
  to identify which project a card referred to.  Below `sm:`, the row
  now stacks: the title (with rocket icon) gets its own full-width
  line, and the status / activity / count badges / progress bar wrap
  underneath as a flex-wrap meta row.  Desktop layout is unchanged.
- **Sidebar: portrait drawer is always fully expanded** — when the
  user collapsed the sidebar on desktop and resized to mobile (or
  loaded mobile with a stale collapsed preference), the slide-in
  drawer kept its `w-72` width but rendered icon-only content inside,
  wasting half the screen with no way to expand because the collapse
  toggle is `hidden md:flex`.  `SidebarCollapsed` is now a read-only
  `Signal<bool>` derived from `!is_mobile && collapsed_state`, where
  `is_mobile` is driven by a `(max-width: 767px)` `MediaQueryList`
  listener in `Layout`.  The desktop collapse preference is preserved
  across resize round-trips.  Children unchanged (they only call
  `.get()`).

---

## [2.3.6] - 2026-04-21

### Security
- **`GET /api/nodes/titles` IDOR fix** — the wiki-link autocomplete
  endpoint previously returned every node title and slug in the database
  to any authenticated user, including Viewers with no permission grants
  on any shared node.  Handler now extracts the caller's claims and
  scopes the query to nodes owned by the caller or explicitly shared
  with them via the `permissions` table.  Admin role bypasses the scope
  filter (unchanged behaviour for admins).

### Changed — Backup / restore trust model
- **Any admin may operate on any backup** — the per-creator ownership
  checks on `GET /admin/backups`, `DELETE /admin/backups/{id}`,
  `GET /admin/backups/{id}/download`, `GET /admin/backups/{id}/preview`,
  and `POST /admin/backups/{id}/restore` have been removed.  The admin
  role is explicitly trusted to repair the entire system, so one admin
  blocking another from restoring a legitimately-created backup was a
  capability gap, not a protection.  `require_admin` still gates every
  handler.
- **`GET /admin/backups` now lists every backup**, not only those
  created by the caller.  Added `BackupRepo::list_all`.
- **Admin backups are exempt from the 5-minute rate limit** — the
  throttle was defence against untrusted-user abuse; admins are
  categorically outside that scope and may legitimately chain backups
  around a risky migration.

### Fixed — Robustness
- **`webhook_dispatch`: reqwest client build failure now aborts the
  dispatch** with a warn log instead of silently falling back to a
  default client with no timeout — a slow webhook receiver on the
  fallback client would pin the tokio task indefinitely.
- **`list_open_for_nodes`: unknown task status / priority now errors**
  instead of silently rendering as Open / Medium on the project
  dashboard — a future migration adding an enum variant would have
  corrupted the dashboard view with the old fallback.

---

## [2.3.5] - 2026-04-21

### Fixed
- **Node editor: Save button reachable on mobile portrait** — the editor
  header was a single `justify-between` row with title + 5-6 controls,
  which overflowed horizontally on narrow viewports so the Save button
  was off-screen right and users had to horizontally scroll to commit
  their edits.  Below `md:`, the header now stacks vertically (title
  row on top, controls row below with `flex-wrap`), keeping Save and
  Cancel always visible without horizontal scrolling.  Desktop layout
  is unchanged.

---

## [2.3.4] - 2026-04-21

### Changed
- **Skeleton loaders replace "Loading…" text** — Suspense fallbacks in
  My Day, Inbox, Notes, Project Dashboard, and NodeView now render a
  pulsing skeleton shaped like the content about to load (rows, cards,
  or article).  Removes the layout jump when data arrives and signals
  that the app is actually working.
- New `ui/src/components/skeleton.rs` module with reusable
  `SkeletonBar`, `SkeletonListRow`, `SkeletonList`, `SkeletonCard`,
  `SkeletonCards`, and `SkeletonArticle` components.

---

## [2.3.3] - 2026-04-21

### Changed
- **Sidebar · Favorites are collapsible per sub-group** — the "Web Links"
  and "Nodes" sub-sections under Favorites each have an independent
  expand/collapse toggle (chevron + count in the header).  Both are
  collapsed by default and state persists per-user in `localStorage`
  under `et.fav.web.expanded` / `et.fav.nodes.expanded`.  Keeps the
  sidebar compact as favorite lists grow.

---

## [2.3.2] - 2026-04-21

### Changed
- **NodeView header: mobile overflow menu** — on narrow viewports the four
  action buttons (Export, Edit, Duplicate, Delete) collapse into a single
  kebab (`⋮`) menu. The header no longer wraps on small screens, and the
  desktop cluster is unchanged at `md:` and above. Click-outside and
  item-click both close the menu; loading / disabled states preserved.
- **CHANGELOG sync** — backfilled releases 2.2.4 → 2.3.1 from internal
  notes and re-anchored ongoing releases to this file.

---

## [2.3.1] - 2026-04-21

### Changed
- **Ghost icon buttons are discoverable** — global `input.css` rule adds
  a subtle hover / focus-visible background to any `<button>` whose only
  child is a Material Symbols icon and that does not already define its
  own background.  Surface small action buttons (edit, delete, pin,
  toggle my-day) that were previously nearly invisible.

---

## [2.3.0] - 2026-04-21

### Changed
- **IA: `/tasks/*` consolidation** — My Day, Inbox, and Calendar are now
  three tabs inside a single Tasks area instead of three sidebar peers.
  New `TasksView` wrapper with a `role="tablist"` tab bar preserves each
  inner view's behaviour unchanged.
  - New URLs: `/tasks/my-day`, `/tasks/inbox`, `/tasks/calendar`.
  - Legacy `/my-day`, `/inbox`, `/calendar` redirect for bookmarks / PWA.
  - PWA `start_url` moved from `/my-day` to `/tasks/my-day`.
  - Service-worker `CACHE_NAME` bumped v3 → v4 to evict pre-consolidation
    bundles on next visit.

---

## [2.2.7] - 2026-04-21

### Added
- **Keyboard focus ring (a11y)** — `:focus-visible` rule in `input.css`
  applies an amber outline to buttons, `[role=button]`, anchors, and
  summary elements.  Fires only for non-pointer focus, so mouse clicks
  never produce a ring.  `!important` wins against per-component
  `focus:outline-none` utilities.

---

## [2.2.6] - 2026-04-21

### Changed
- **Dashboard sort: most-recently-active projects first** —
  `ProjectDashboardEntry.last_activity_at = MAX(node.updated_at, MAX(tasks.updated_at))`
  surfaces the 2–3 projects currently in flight at the top of the
  dashboard.  Each card displays a compact "Updated 3h ago" label
  (hidden below `sm:` to preserve the mobile layout).

### Added
- `TaskRepo::max_task_updated_for_nodes` method; `format_relative_short`
  UI helper in `format_helpers.rs`.

---

## [2.2.5] - 2026-04-21

### Removed
- **Quick Add floating action button** — the amber FAB in the lower-right
  was rarely used and often occluded content.  Quick capture remains
  accessible via the `n` keyboard shortcut.

---

## [2.2.4] - 2026-04-18

### Security
- **rustls-webpki name-constraint bypass** — bumped `rustls-webpki`
  0.103.10 → 0.103.12 to resolve RUSTSEC-2026-0098 / 0099.  Bumped the
  `aws-smithy-runtime` stack to the latest as a follow-up.
- **cargo-audit hygiene** — expanded `ignore` list with dated rationale
  for the remaining transitive 0.101.7 path (AWS endpoints only, rustls
  0.21).  CI is green on a clean advisory database.

### Documentation
- CLAUDE.md: release is not done until every workflow on the pushed ref
  is green; a successful `Release` alongside a red `CI` still leaves
  `master` broken for the next merge.

---

> Note: releases 1.76 through 2.2.3 are tracked in the internal release
> notes (see `.claude/MEMORY.md`) rather than this file. Major themes
> during that period: backup/restore with schema v2 (2.1.0), enhanced
> dashboard with project status and open tasks (2.2.0), security
> hardening sprints (1.95 → 1.98), PWA offline (1.93.0), graph-view
> improvements, task panel refactor, and multi-user permissions.

---

## [1.75.12] - 2026-04-06

### Fixed
- **Graph view: Auto-arrange now persists positions to the database** — added a batch `PUT /graph/positions` endpoint so all node positions are saved in a single transaction after auto-arrange runs.

---

## [1.75.11] - 2026-04-06

### Changed
- **Housekeeping**: removed dead `force_layout()` function, updated module doc comment, bumped `api` version to match current release, standardized `edition = "2024"` across all crates, added CHANGELOG gap note for versions 1.52.0–1.75.3.

---

## [1.75.10] - 2026-04-05

### Changed
- **UI: unified save/cancel buttons across all sections** — replaced text-label buttons with consistent icon-only buttons (`check` for save, `close` for cancel) everywhere:
  - Task panel, My Day view, Note panel, Links panel, Tag Manager, Templates view
- **UI: unified add/cancel toggles** — section header "Add" buttons now use icon-only (`add` ↔ `close`) in Task panel, Note panel, Tag Manager, and Templates view
- All icon buttons share the same visual language: `p-1.5 rounded-lg`, green hover for save, stone hover for cancel

---

## [1.75.9] - 2026-04-05

### Fixed
- **Graph view: Auto-arrange now centers the graph in the viewport** — removed the force simulation that was pushing nodes far apart. The hierarchical BFS layering alone produces a clean, non-overlapping layout instantly.
  - Nodes are now centered in the viewport after auto-arrange (not anchored to a corner)
  - Minimum zoom is 0.5x so nodes stay readable at any graph size
  - Disconnected components are tiled in a grid with proper spacing
  - Computation is now near-instant (no 300-iteration force loop)

---

## [1.75.8] - 2026-04-05

### Changed
- **Graph view: unified toolbar design** — all controls (Add Edge, Fit, Auto-arrange, zoom) are now in a single cohesive container with consistent height, dividers, and visual treatment.
- **Graph view: manual zoom input** — the zoom percentage is now an editable number field. Type any value (e.g. `100` for 100%) and press Enter to set it exactly. The field syncs bidirectionally with wheel and pinch-to-zoom gestures.

---

## [1.75.7] - 2026-04-05

### Fixed
- **Graph view: tighten auto-arrange spacing** — nodes now cluster closer together with reduced spacing constants (120→80px horizontal, 110→90px vertical), stronger edge attraction, and weaker repulsion. Layout anchored to upper-left corner instead of centered for immediate visibility.

---

## [1.75.6] - 2026-04-05

### Added
- **Graph view: Auto-arrange button** — smart layout algorithm that re-arranges all nodes to eliminate overlap (shapes + titles + tag dots) with optimal spacing for readability.
  - **Hierarchical placement** — root nodes (no incoming edges) placed in a top row, then BFS layers fan out below; hubs sorted toward the center of each layer.
  - **Multi-component support** — disconnected subgraphs are arranged in a grid, each independently laid out.
  - **Enhanced force refinement** — envelope-based repulsion prevents text overlap, same-type nodes get extra separation, component separation force keeps subgraphs apart.
  - **Auto-fit viewport** — after layout, pan and zoom automatically adjust to frame all nodes.
  - **Progress spinner** — full-screen overlay with animated spinner and message during computation.

---

## [1.75.5] - 2026-04-05

### Changed
- **Graph view: significantly expanded work area** — virtual canvas enlarged from 1000×700 to 3000×2000 (~6× more space) with proportionally scaled margins and minimap.
- **Graph view: auto-grow canvas** — force layout bounds now dynamically expand based on node count (up to 4× for 200+ nodes), so the canvas grows with your database.
- **Graph view: "Re-layout" button** — new toolbar button that re-runs the force-directed simulation to spread nodes apart when the graph gets crowded.
- **Graph view: wider zoom range** — zoom out to 0.05× (was 0.1×) and zoom in to 16× (was 8×) for finer control over large graphs.

---

## [1.75.4] - 2026-04-05

### Changed
- **CI/CD: migrate GitHub Actions to Node.js 24-compatible versions** — upgraded `actions/checkout` v4→v6, `docker/build-push-action` v6→v7, `docker/login-action` v3→v4, `docker/setup-buildx-action` v3→v4 to eliminate Node.js 20 deprecation warnings.

---

<!-- Note: versions 1.52.0–1.75.3 (24 releases) are documented in git commit history: https://github.com/jchultarsky/ember-trove/tags -->

---

## [1.51.0] - 2026-03-29

### Added
- **Calendar view** — new sidebar entry (between My Day and Dashboard) showing a month grid of tasks that have a due date. Navigate forward/backward by month with chevron buttons or jump to the current month with "Today". Each day cell shows colour-coded chips (priority tint + text) for its tasks; done/cancelled tasks are struck through. Clicking a chip opens the node detail view. Today's cell is highlighted with an amber ring. The grid is Mon–Sun with leading blank cells for offset days.
- **`GET /api/calendar?year={y}&month={m}`** endpoint — returns `Vec<MyDayTask>` for tasks whose `due_date` falls within the given calendar month. Accessible to any authenticated user; results scoped to the caller's own tasks.

---

## [1.50.1] - 2026-03-29

### Fixed
- **Task edit form consistency** — the inline edit form in `TaskPanel` previously only allowed changing the title. It now also exposes a priority `<select>` (Low / Medium / High) and a `<input type="date">` for the due date, matching the fields available when creating a task. All three fields are saved in a single `UpdateTaskRequest`.

---

## [1.50.0] - 2026-03-29

### Fixed
- **My Day carry-over** — tasks previously disappeared from "My Day" when the date rolled over to a new day. The query now returns tasks whose `focus_date` is on or before today, unless the task is already `done` or `cancelled`. Incomplete tasks from prior days are carried forward automatically until marked done or removed from My Day. A small history-icon badge shows the original focus date for carried-over tasks.

---

## [1.49.1] - 2026-03-29

### Fixed
- **Admin `is_owner` in NodeView** — `is_owner` is now `true` when the authenticated user carries the `"admin"` role, regardless of who created the node. Previously admin users saw no "Add note", "Edit permissions", or "Pin" controls on nodes they did not own. Computed as `user.sub == n.owner_id || user.roles.contains("admin")` using the `roles: Vec<String>` field already present in `UserInfo`.

---

## [1.49.0] - 2026-03-29

### Added
- **Drag-and-drop image upload in Markdown editor** — drag one or more image files onto the editor textarea to upload them inline. The file is sent to the existing `POST /nodes/{id}/attachments` endpoint and the resulting URL is inserted as `![filename](url)` at the cursor position. A `![uploading-N…]()` placeholder is inserted immediately while the upload is in-flight and replaced (or removed on failure) once the request completes. An amber inset ring appears on the textarea during drag-over. Only `image/*` MIME types are accepted; non-image files are silently skipped.
- **Clipboard paste image upload** — `Ctrl+V` / `Cmd+V` with an image on the clipboard (e.g. a screenshot) triggers the same upload pipeline. `ev.prevent_default()` is called only when at least one image item is found in the clipboard data, so text paste is unaffected.
- A "Uploading image…" spinner badge appears in the top-right corner of the editor pane while any upload is in progress (`img_uploading: RwSignal<bool>`).

---

## [1.48.2] - 2026-03-29

### Fixed
- **Admin sees all nodes in list view** — `list_nodes` was always setting `params.subject_id = Some(claims.sub)`, which restricts results to nodes the caller owns or holds an explicit permission row for. Admin users now skip this filter (`subject_id` left as `None`), causing the SQL `IN (SELECT node_id FROM permissions …)` clause to be omitted entirely and all nodes to be returned.

---

## [1.48.1] - 2026-03-29

### Fixed
- **Admin bypasses per-node permission check** — `require_role()` in `api/src/auth/permissions.rs` now returns `Ok(())` immediately when the caller's JWT contains `"admin"` in its `roles` claim (populated from Cognito `cognito:groups`). Previously an admin user received 403 when opening any node they had not explicitly been granted a permission row for.

---

## [1.48.0] - 2026-03-27

### Added
- **Graph minimap** — small 160×112 px overview panel fixed at the bottom-right corner of the graph view. Shows all node positions as colour-coded dots (matching the node-type fill colours), faint edge lines, and an amber viewport indicator rect that reflects the current pan/zoom state. Clicking anywhere on the minimap pans the main graph to centre on that graph coordinate. The panel is hidden while the graph is loading or empty. Implemented using four new constants (`MINI_W`, `MINI_H`, `MINI_SCALE_X`, `MINI_SCALE_Y`) and a reactive `{move || {}}` block; the viewport rect updates via inner reactive closures so pan/zoom changes update only those SVG attributes without re-rendering the full minimap.

---

## [1.47.0] - 2026-03-27

### Added
- **Graph edge delete** — hovering an edge now shows a red "Delete edge" button at the bottom of the hover card. Clicking it calls `DELETE /api/edges/{id}` and removes the edge from the graph reactively without a page reload.
- **Add Edge mode in graph** — new "Add Edge" toolbar button (top-right, amber when active). Click it to enter edge-create mode (cursor → crosshair). Click a source node (amber dashed ring appears), then a target node to open a type-picker popup (edge type select + optional label). Confirm to create the edge immediately. Node dragging is disabled while in this mode; Cancel or clicking the toolbar button again exits.
- **Edge count badge on node cards** — nodes that participate in at least one edge now show a `link` icon + count badge below the date in the card's top-right corner. `Node` DTO gains `edge_count: u32`; the `list_nodes` SQL query uses a `LEFT JOIN` subquery to count edges (source OR target) per node.

---

## [1.46.0] - 2026-03-27

### Added
- **Template picker in quick-capture modal** — the FAB / `n`-shortcut modal now shows a "Template (optional)" select alongside the Type select. Choosing a template pre-fills the Notes textarea and sets the node type to match; `template_id` is passed in `CreateNodeRequest` for activity-log attribution.
- **Template picker in node editor (create mode)** — a compact "— Template —" select appears in the node editor header only when creating a new node. Selecting a template overwrites body and type. Both pickers use `LocalResource<Vec<NodeTemplate>>` mirrored into an `RwSignal` for untracked reads in `on:change` closures.

---

## [1.45.3] - 2026-03-27

### Changed
- **Node card body preview expanded to 3 lines** — CSS class changed from `truncate` (1 line) to `line-clamp-3`; `body_preview` character cap raised from 120 to 300 to ensure 3 lines of text are available at typical card widths.

---

## [1.45.2] - 2026-03-27

### Changed
- Documentation update: README, CHANGELOG, `docs/deploy-aws.md`, and `CLAUDE.md` updated with session learnings (boto3 Cognito CSS application, SVG z-order, `pointer-events`, newtype context pattern, Cognito CSS allowed-class list).

---

## [1.45.1] - 2026-03-27

### Changed
- **`n` keyboard shortcut now opens quick-capture modal** — previously `n` navigated to the full NodeEditor (`View::NodeCreate`); now it opens the same lightweight `CreateNodeModal` as the FAB, making both entry points consistent. `ShowCapture` context signal lifted to the App root so the keyboard handler and Layout share state without prop-drilling.

---

## [1.45.0] - 2026-03-27

### Added
- **Graph tag filter** — clicking a coloured tag dot on any graph node filters the graph to show only nodes that share that tag (and their connecting edges). The active dot renders larger with an amber stroke. A "Tag filter active · ×" row appears in the legend panel to clear the filter. Clicking the same dot again also clears it. Tag filter combines with the existing type-filter toggles.

---

## [1.44.1] - 2026-03-27

### Fixed
- **Graph tag dots hidden by title pill** — tag dots were rendered at `cy+27`, inside the title background pill (`cy+22` to `cy+36`), causing the pill to paint over them. Fixed by moving dots to `cy+42` (below the pill's bottom edge) and rendering the dot block after the title `<text>` element in SVG order so they always paint on top.

---

## [1.44.0] - 2026-03-27

### Added
- **Node-type icons on graph shapes** — Material Symbols Outlined ligature centred on each node shape (white, semi-transparent, `pointer-events: none`). Uses the same `type_icon()` helper as the sidebar and node lists. SVG `style=` attribute used to avoid Leptos 0.8 `attr:` prefix serialisation bug.

---

## [1.43.0] - 2026-03-27

### Added
- **Graph view tag colour overlay** — up to 5 small filled dots (r=3.5, white outline) rendered below each node shape, one per tag, using the tag's hex colour. Dots are horizontally centred and spaced 9 px apart. No backend changes required.

---

## [1.42.0] - 2026-03-27

### Added
- **Collapsible markdown preview in node editor** — the live preview pane can be toggled via a visibility icon button in the editor header. Initial visibility is determined from `window.innerWidth` (≥ 768 px → visible; mobile → hidden by default). Toggle state stored in `show_preview: RwSignal<bool>`. Amber styling on the button when preview is active.

---

## [1.41.0] - 2026-03-27

### Added
- **Saved search presets** — migration 017 adds `search_presets` table (owner-scoped). New DTOs: `SearchPresetId`, `SearchPreset`, `CreateSearchPresetRequest` in `common`. New repo: `SearchPresetRepo` / `PgSearchPresetRepo`. Routes: `GET /api/search-presets`, `POST /api/search-presets`, `DELETE /api/search-presets/{id}`. UI: "Presets ▾" dropdown in the SearchView filter bar — load a preset to restore all filters, delete with ×, or save the current search via an inline form. Total tests: 55.

---

## [1.40.0] - 2026-03-27

### Added
- **Node tagging from list view** — each node card in the list view now has a tag-picker dropdown. All tags are fetched once per list render; per-card `show_picker: RwSignal<bool>` controls visibility. Dropdown shows a colour swatch, tag name, and an amber checkmark for applied tags. Clicking attaches or detaches the tag immediately and refreshes the list. Fixes attachment drop-zone compile error by adding `DragEvent` and `DataTransfer` to web-sys features.

---

## [1.39.0] - 2026-03-27

### Added
- **Graph pinned-node highlight** — an amber hollow ring (`stroke: #f59e0b`, r=29) is drawn behind the node shape for pinned nodes, making them visually distinct in the graph view.

---

## [1.38.0] - 2026-03-27

### Added
- **`p` keyboard shortcut to toggle pin** — pressing `p` while a node detail is open toggles the node's pinned state (same as the pin button in the toolbar). `current_node_pinned: RwSignal<bool>` context is provided from the App root; `NodeView` writes it on load and keeps it in sync. Toast feedback. `ShortcutsModal` updated.

---

## [1.37.0] - 2026-03-27

### Changed
- **Attachment bulk upload** — the single-file picker is replaced by a drag-and-drop drop zone accepting multiple files simultaneously. Files are uploaded sequentially with a live `n/total` progress counter. A clear button resets the pending queue. No backend changes.

---

## [1.36.0] - 2026-03-27

### Added
- **Node pinning** — migration 016 adds `pinned BOOLEAN DEFAULT FALSE` to the `nodes` table. `PUT /api/nodes/{id}/pin` toggles pin state (owner-only). Node list sorted `pinned DESC, updated_at DESC`. Amber `push_pin` icon on pinned cards. Pin toggle button in the node-detail header.

---

## [1.35.0] - 2026-03-27

### Changed
- **Search ranking improvements** — `ts_rank_cd` now uses length normalisation (`|1`) so long documents do not unfairly dominate results. Fuzzy (ILIKE-only) body matches receive a 0.05 rank floor to distinguish them from zero-score results. The `12%` raw relevance figure in SearchView is replaced with a 3-bar visual indicator.

---

## [1.34.0] - 2026-03-27

### Fixed
- **Notes panel scrolling** — notes list now has `max-h-[28rem] overflow-y-auto` so long note histories scroll within the panel instead of expanding the page. A note-count badge is shown next to the panel header.
- **CI test stability** — `AppState` in tests now uses `..Config::default()` to avoid compilation failures when `Config` gains new fields.

---

## [1.33.0] - 2026-03-27

### Added
- **Bulk permission management** — new "Bulk Permissions" view in the admin sidebar. Groups all permission rows across all nodes; supports inline role-change and revoke; resolves Cognito usernames for display; filter input for large permission lists; owner rows are read-only.

---

## [1.32.0] - 2026-03-27

### Added
- **Node templates** — migration 015 adds `node_templates` table. CRUD routes at `/api/templates`. `TemplatesView` in sidebar with inline Markdown editor and "Use" button. `TemplatePrefill` context pre-fills `NodeEditor` when creating a node from a template. Activity action `CreatedFromTemplate` recorded on use.

---

## [1.31.0] - 2026-03-27

### Added
- **Keyboard shortcuts help modal** — pressing `?` toggles an overlay listing all global shortcuts. Escape also closes it. Rendered via Leptos `<Portal>` (`ShortcutsModal` component).

---

## [1.30.0] - 2026-03-27

### Added
- **Node version history** — migration 014 adds `node_versions` table. `NodeVersionRepo` / `PgNodeVersionRepo` snapshot the node body on every save (fire-and-forget). Routes: `GET /api/nodes/{id}/versions`, `POST /api/nodes/{id}/versions/{vid}/restore`. `VersionPanel` collapsible timeline UI in the node-detail view.

---

## [1.29.0] - 2026-03-27

### Added
- **Activity / audit log** — migration 013 adds `node_activity` table. `ActivityAction` enum with 10 variants (Created, Updated, Published, Archived, TagAttached, TagDetached, PermissionGranted, PermissionRevoked, AttachmentUploaded, AttachmentDeleted). `GET /api/nodes/{id}/activity` returns a timestamped log. `ActivityPanel` collapsible timeline UI in the node-detail view. All mutating route handlers instrumented.

---

## [1.28.0] - 2026-03-25

### Added
- **Node export** — `GET /nodes/{id}/export?format=markdown|json` returns a file download. Markdown includes YAML front-matter (title, type, status, tags, timestamps). JSON serialises the full Node DTO. A download icon in the node-view toolbar triggers the browser's native save dialog.
- **Public sharing links** — owners can generate opaque share tokens (`POST /nodes/{id}/share`). Sharing a token URL (`/share/<token>`) renders a read-only public node view with no login required. Tokens can be listed and revoked from the new "Public Links" panel in the node view. Migration 012 adds the `share_tokens` table (with optional `expires_at`).

## [1.27.0] - 2026-03-25

### Added
- **SES invite notification** — when an existing Cognito user is granted access to a node, an HTML+text email is sent via AWS SES v2 with the node title, role, and a direct link. New users continue to receive only the Cognito welcome email (no duplicate). Controlled by the optional `SES_FROM_EMAIL` env var; if unset the invite still works, the email is simply skipped. Send failures are logged as warnings and do not affect the API response.
- **Global keyboard shortcuts** — `n` new node · `g` graph · `/` search · `Esc` back to node list. Suppressed inside inputs, textareas, selects, contenteditable elements, and when Ctrl/Meta/Alt is held.

## [1.26.0] - 2026-03-25

### Added
- **GitHub CD automation** — `LIGHTSAIL_HOST`, `LIGHTSAIL_SSH_KEY` secrets and `DEPLOY_ENABLED=true` repository variable are now set. Every push of a `v*.*.*` tag triggers the existing `release.yml` workflow: creates a GitHub Release, SSH-builds the Docker images on the EC2 host, force-recreates the containers, and health-checks the API. No more manual deploy steps.

### Fixed
- **Permission panel ownership gating** — `PermissionPanel` now accepts `is_owner: bool`; the invite button, role-change dropdown, and revoke button are hidden for viewers and editors (they only see a read-only role badge).
- **`is_owner` computation** — `node_view.rs` previously treated every authenticated user as owner. It now correctly compares `auth.sub == node.owner_id`.
- **Revoke button visibility** — Replaced the unreliable `opacity-0 group-hover:opacity-100` pattern (broken in Tailwind v4) with an always-visible muted `text-stone-300 hover:text-red-500` style, consistent with the note-edit button fix in v1.24.1.

## [1.24.1] - 2026-03-24

### Fixed
- **Note edit button always visible** — Replaced `opacity-0 group-hover:opacity-100` CSS pattern (unreliable in Tailwind v4 due to `@media (hover:hover)` scoping) with an always-rendered button in muted `stone-300` that brightens to `amber-500` on hover. The pencil icon is now permanently visible on every note card.

## [1.24.0] - 2026-03-24

### Added
- **Editable notes** — Notes can now be edited after creation. Each note in the panel shows a pencil icon on hover (owner only); clicking it switches to an inline textarea with Save / Cancel controls and Ctrl+Enter shortcut. The API gains `PATCH /notes/:id` (owner-scoped); the `Note` DTO gains `updated_at`; notes display a `· edited` badge when `updated_at` differs from `created_at` by more than 2 seconds. Migration `010_notes_updated_at.sql` adds the column + trigger and back-fills existing rows from `created_at`.
- **Editable task titles** — Each task row gains an edit pencil icon in its hover-action strip. Clicking it replaces the title with an inline input; Enter saves via `PATCH /tasks/:id`, Escape cancels. All reactive closures capture only `Copy` signal types to stay `FnMut`-compatible with Leptos 0.8.

### Changed
- Notes are returned newest-first by the API (`ORDER BY created_at DESC`) — the panel now displays them in that order (most recent at the top).

## [1.23.0] - 2026-03-24

### Fixed
- **Portal modals** — `DeleteConfirmModal` and `LinkPickerModal` now use Leptos `<Portal>` (same fix as v1.22.0 for `AddFavoriteModal`). Both were rendered inside ancestor elements that could carry a CSS `transform`, trapping their `position:fixed` backdrops.

### Changed
- **Permission panel — inline role editing** — Each permission row in the "Sharing" section now shows an inline `<select>` dropdown (owner / editor / viewer) instead of a static badge. Changing the role calls `PUT /permissions/{id}` immediately, with a "saving…" state while the request is in flight. The `update_permission` API helper was added to `ui/src/api.rs`.

### Added
- **API integration tests** — `api/src/tests.rs` contains 36 router-level integration tests run via `tower::ServiceExt::oneshot` with stub repositories and a lazy pool (no live database required). Tests cover: health endpoint shape, route registration for every domain (nodes, edges, tags, search, graph, notes, favorites, permissions — standalone and per-node), auth-guard behaviour, and permission DTO serialisation. Total test count: **63** (41 API + 22 common).

## [1.22.0] - 2026-03-24

### Fixed
- **Add-Favorite dialog confined to sidebar**: The "Add to Favorites" modal was rendered inside the sidebar's `<aside>` DOM node, which carries a CSS `translate-x-*` transform for the mobile slide-in animation. Even with `md:transform-none`, the transform created a new stacking context that trapped `position:fixed` children inside the sidebar's bounding box (~230 px wide), making the dialog unusable — especially in collapsed mode. Fixed by wrapping the modal backdrop in Leptos 0.8's `<Portal>`, which teleports the DOM nodes to `<body>`, completely bypassing any ancestor stacking context.

## [1.21.2] - 2026-03-23

### Fixed
- **Health-check tooling missing from runtime image**: `debian:trixie-slim` does not include `wget`; `docker exec deploy-api-1 wget …` always exited non-zero, causing every production deploy to fail at the verification step. Added `wget` to the `apt-get install` list in the API runtime stage so the deploy health-check command works as intended.

## [1.21.1] - 2026-03-23

### Fixed
- **Health endpoint rate-limiting**: `/api/health` is now exempt from the `tower_governor` rate-limit layer. Monitoring tools and the deploy health-check (`wget` inside the API container) connect directly without nginx headers, which caused the rate-limiter key extraction to fail and return 500, making every production deploy appear unhealthy. The health route is now handled by a separate sub-router that does not pass through `GovernorLayer`.

## [1.21.0] - 2026-03-24

### Added
- **Standalone permission routes**: `GET /api/permissions[?node_id=<uuid>]` lists all grants (optionally filtered to a node); `PUT /api/permissions/{id}` updates the role on an existing grant; `DELETE /api/permissions/{id}` revokes a grant by ID directly — complementing the existing nested routes under `/api/nodes/{id}/permissions`.
- **`UpdatePermissionRequest` DTO** and **`PermissionListParams` DTO** added to the `common` crate.
- **`list_all` and `update` methods** added to `PermissionRepo` trait and `PgPermissionRepo`.
- **Rate limiting** via `tower_governor 0.8`: 10 requests/second per peer IP (burst cap 100) applied globally to all API routes. A background task prunes stale IP entries every 60 seconds.
- **Unit test suite expansion**: 16 new tests — permission repo helper round-trips, governor config validity, and DTO serde/validation in `common`.

## [1.20.2] - 2026-03-24

### Fixed
- **502 Bad Gateway on login in local Docker stack**: nginx's default 4 KB `proxy_buffer_size` was too small for the `/api/auth/callback` response, which sets large `Set-Cookie` headers containing JWT access/id/refresh tokens. Increased `proxy_buffer_size` and `proxy_buffers` to 32 KB in `deploy/nginx.conf`.

## [1.20.1] - 2026-03-24

### Fixed
- **Production deploy health check**: replaced fixed `sleep 10` with a 5 s × 12 retry loop (up to 60 s total). The API container starts quickly but OIDC discovery and database migrations take 5–15 s; the fixed sleep was not sufficient, causing false-negative deploy failures even when the deployment itself succeeded.

## [1.20.0] - 2026-03-23

### Added
- **Local development workflow**: `docker-compose.yml` now supports a fully self-contained local stack with one command:
  `docker compose -f deploy/docker-compose.yml --env-file deploy/.env.local up --build`
- **`minio-init` service**: auto-creates the `ember-trove` S3 bucket on first boot so attachment uploads work without any manual MinIO setup.
- **`deploy/.env.local.example`**: committed template documenting the three variables that need real values (`OIDC_CLIENT_SECRET`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`).
- **Cognito localhost callback**: registered `http://localhost:8003/api/auth/callback` and `http://localhost:8003` as allowed redirect/logout URLs so Cognito OIDC authentication works in the local Docker stack.

### Fixed
- **`API_EXTERNAL_URL` for local dev**: corrected from `:3003` (direct API port) to `:8003` (nginx proxy) so OIDC redirect URIs match the registered Cognito callback.
- **`cargo audit` paste warning silenced**: `RUSTSEC-2024-0436` (`paste` unmaintained, warning-level only via Leptos transitive dep) added to ignore list — Leptos owns that upgrade path.
- **`tar` 0.4.45 in `Cargo.lock`**: carried forward from v1.19.1 patch.

## [1.19.3] - 2026-03-23

### Fixed
- **Deploy concurrency guard**: added `concurrency: group: production-deploy, cancel-in-progress: true` to `release.yml` so rapid successive tag pushes no longer pile up concurrent Docker builds on the Lightsail VM.

## [1.19.2] - 2026-03-23

### Fixed
- **Production deploy timeout extended to 60 minutes**: Rust rebuild on a cold Lightsail VM regularly exceeded the previous 30-minute SSH timeout, causing deploy failures even when the build was progressing normally.

## [1.19.1] - 2026-03-23

### Fixed
- **Patched `tar` 0.4.44→0.4.45** (RUSTSEC-2026-0067: `unpack_in` symlink chmod; RUSTSEC-2026-0068: PAX size header parsing — both medium severity).

## [1.19.0] - 2026-03-23

### Added
- **`cargo audit` job in CI**: scans `Cargo.lock` against the RustSec advisory database on every push; blocks merges when fixable vulnerabilities are present.
- **Migration validation job in CI**: runs `sqlx migrate run` against an ephemeral Postgres 16 service container on every push to catch SQL errors before deploy.
- **Docker build validation job in CI**: builds both `api` and `ui` images (no push) using GitHub Actions layer cache to catch `Dockerfile` errors in CI.
- **Automated production deploy in `release.yml`**: pushing a version tag now SSHs into the Lightsail server, rebuilds images, restarts services, and verifies health — controlled by the `DEPLOY_ENABLED` repository variable.

### Fixed
- **`release.yml` no longer fails on every branch push**: the `secrets` context is not valid in job-level `if` conditions; switched to `vars.DEPLOY_ENABLED` (repository variables are allowed at job level).
- **"Add to Favorites" dialog now centers on the full screen**: Tailwind's `translate-x-0` left a `transform: translateX(0)` on the sidebar even on desktop, creating a CSS stacking context that trapped `position: fixed` overlays inside the sidebar bounds. Added `md:transform-none` to remove the transform at the desktop breakpoint; mobile slide animation is unaffected.
- **Patched `aws-lc-sys` 0.38→0.39** (RUSTSEC-2026-0048/0044, high severity) and **`rustls-webpki` 0.103.9→0.103.10** (RUSTSEC-2026-0049).

### Changed
- **Rust toolchain pinned to 1.92** via `rust-toolchain.toml` for reproducible CI builds (AWS SDK requires ≥ 1.91.1).
- **GitHub Actions opted into Node.js 24** via `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true`; removes deprecation warnings ahead of GitHub's June 2026 forced migration.

## [1.18.0] - 2026-03-22

### Fixed
- **PKCE (S256) added to OIDC login flow**: Cognito app clients created after November 2024 silently reject token exchanges without PKCE (`invalid_grant`). Login now generates a `code_verifier` (32 random bytes, URL-safe base64), derives `code_challenge = BASE64URL(SHA256(verifier))`, and stores the verifier in a `SameSite=Lax; HttpOnly; Secure; path=/api/auth/callback` cookie consumed once in the callback handler.
- **Blank page after login on production**: Two root causes resolved:
  - CSP `script-src` was missing `'unsafe-inline'`, which silently blocked Trunk's inline `<script type="module">` bootstrap. Added `'unsafe-inline'` to `nginx.prod.conf`.
  - `WebAssembly.instantiateStreaming` hung indefinitely on the nginx reverse-proxy + preload-hints configuration. Added a regular (non-module) `<script>` patch to `ui/index.html` that replaces `instantiateStreaming` with an `arrayBuffer()` fallback before Trunk's module bootstrap runs.

## [1.17.0] - 2026-03-22

### Added
- **`version` and `timestamp` fields on `GET /health`**: health response now includes the running binary version and a UTC timestamp, enabling CI/CD pipelines to verify the deployed version without admin credentials.
- **30-second request timeout**: all API requests now return `408 Request Timeout` if processing exceeds 30 seconds, preventing hung connections under load.
- **`X-Request-Id` middleware**: every response carries a `X-Request-Id` UUID header (generated server-side if not provided by the client) for distributed tracing and log correlation. Header is exposed in CORS so browser clients can read it.

### Changed
- Updated `tower-http` workspace dependency to enable `timeout`, `request-id`, and `propagate-header` features.
- Stale doc comment in `AuthClaims.roles` updated to reference Cognito groups instead of Keycloak realm roles.

## [1.16.0] - 2026-03-21

### Added
- **Unit test coverage**: 27 tests total (up from 9).
  - `common::admin` — 8 tests for `AdminUser::display_name()` and `CreateAdminUserRequest` validation.
  - `common::auth` — 4 tests for `UserInfo::from(AuthClaims)`, serde round-trip, and `#[serde(default)]` on `roles`.
  - `api::wikilink` — 7 edge-case tests (whitespace trimming, empty targets, pipe with empty target, duplicates, adjacent links).

## [1.15.0] - 2026-03-21

### Added
- **Operational metrics endpoint**: `GET /api/metrics` (admin-only) returns a JSON snapshot for monitoring:
  - `version` — API binary version.
  - `uptime_secs` — process uptime since last restart.
  - `db.pool_size` / `db.pool_idle` — PostgreSQL connection pool utilisation.
  - `counts.*` — row counts for `nodes`, `edges`, `tags`, `notes`, `tasks`, `attachments`, `user_favorites`.
- `AppState` now records `started_at: Instant` for uptime tracking.

### Fixed
- Removed unused `post` import from `api/src/routes/favorites.rs`.

## [1.14.0] - 2026-03-21

### Changed
- **Admin user management migrated to Amazon Cognito**: replaced Keycloak Admin REST API client (`keycloak.rs`) with `CognitoAdminClient` (`cognito.rs`) backed by the AWS SDK.
  - All admin endpoints (`GET/POST /api/admin/users`, `DELETE /api/admin/users/{id}`, `PUT /api/admin/users/{id}/roles`, `GET /api/admin/users/roles`) now operate against the Cognito User Pool.
  - Users are identified by email; Cognito groups replace Keycloak realm roles.
  - `CreateAdminUserRequest` no longer requires a separate `username` field — email is used as the Cognito username.
  - Welcome email uses Cognito's built-in `AdminCreateUser` invite flow.
  - Dead `api/src/admin/keycloak.rs` removed.

## [1.13.0] - 2026-03-21

### Added
- **Automated backup script** (`deploy/backup.sh`): streams `pg_dump` output through gzip and uploads directly to S3-compatible object storage in a single pipeline.
  - `list` subcommand shows all stored backups.
  - `restore <file>` subcommand streams a backup from S3 back into PostgreSQL.
  - Auto-prunes oldest backups once count exceeds `BACKUP_RETAIN` (default 30).
  - Reads `deploy/.env.prod` automatically; all vars overridable via environment.
  - Supports custom `S3_ENDPOINT` for MinIO/Lightsail Object Storage.
  - Cron example: `0 2 * * * /home/ubuntu/ember-trove/deploy/backup.sh >> /var/log/ember-backup.log 2>&1`

## [1.12.0] - 2026-03-21

### Added
- **Graph type-filter**: each node type in the legend is now a clickable toggle. Clicking hides/shows all nodes of that type (dims to 40% with a "hidden" badge). Edges are automatically hidden when either endpoint type is filtered out.
- **Graph "Fit" button**: toolbar button (top-right of graph view) resets pan and zoom to the default view, bringing all nodes back into frame.

## [1.11.0] - 2026-03-21

### Added
- **Inline attachment preview**: images (any `image/*` type) and PDFs render inline inside the Attachments panel via a toggle eye-icon button.
  - Images: `<img>` with `max-h-96 object-contain` — respects aspect ratio, fits any width.
  - PDFs: `<iframe>` at 500 px height for in-page browsing.
  - Download and delete buttons remain visible for all attachment types.

### Fixed
- Clippy `collapsible_if` warnings in `favorites_section` resolved.
- "Favorites" section header in dark mode uses `stone-400` for better legibility.

## [1.10.0] - 2026-03-21

### Added
- **Sidebar Favorites**: pin any internal node or external URL to the sidebar for one-click access.
  - Favorites section sits between the search bar and "All Nodes", visible in both expanded and collapsed sidebar modes.
  - Add favorites via an in-modal picker: "Internal Node" tab (live search + select) or "External URL" tab (URL + label inputs).
  - Node favorites navigate to the node's detail view on click; URL favorites open in a new browser tab.
  - Reorder favorites with up/down arrow buttons (visible on hover).
  - Remove any favorite with the trash icon (visible on hover).
  - Favorites are user-scoped and persisted in PostgreSQL (`user_favorites` table, migration `009_favorites.sql`).
  - New API endpoints: `GET /api/favorites`, `POST /api/favorites`, `DELETE /api/favorites/{id}`, `PATCH /api/favorites/reorder`.

## [1.9.2] - 2026-03-19

### Fixed
- **Username display**: sidebar now falls back to `email` before `sub` UUID when the identity provider does not populate the `name` claim (Cognito default behaviour).
- **Cognito logout loop**: logout handler now redirects through Cognito's `end_session_endpoint` with `logout_uri`, clearing the Cognito SSO session cookie so the browser lands on the login page instead of immediately re-authenticating.
- **nginx proxy buffer**: raised `proxy_buffer_size` to 128 KB in `nginx.prod.conf` to accommodate large JWT `Set-Cookie` headers that exceeded the default 4 KB buffer and caused `502 Bad Gateway` on `/api/auth/callback`.

## [1.9.1] - 2026-03-19

### Added
- **Production AWS stack**: `deploy/docker-compose.prod.yml` — four-service compose (postgres, api, ui, nginx proxy) with `COOKIE_SECURE=true` and Cognito / Lightsail Object Storage environment variables.
- **Production nginx config**: `deploy/nginx.prod.conf` — TLS termination (Let's Encrypt), HSTS header, ACME challenge passthrough, and generous proxy buffers for JWT headers.
- **Env template**: `deploy/.env.prod.template` with documented placeholders for all production secrets.
- **AWS deployment guide**: `docs/deploy-aws.md` — step-by-step guide covering Lightsail, Route 53, Cognito, Object Storage, IAM, Certbot, and auto-renewal.

### Changed
- Replaced Keycloak with **Amazon Cognito** as the production identity provider. Local development continues to use Keycloak via `docker-compose.yml`.

## [1.9.0] - 2026-03-18

### Added
- **JWT expiry redirect**: `parse_json` helper now redirects to the login page when both the access token and refresh token are expired, instead of looping on 401.
- **Single-user mode**: node list, tag list, and notes feed return all data regardless of `owner_id`; any authenticated user can add notes to any node.
- **Mobile-responsive layout**: hamburger top bar on narrow viewports; sidebar slides in as a full-height overlay with a backdrop dismiss.

## [1.8.0] - 2026-03-18

### Added
- **Backchannel logout**: Keycloak logout now revokes the refresh token server-side via the OIDC revocation endpoint, preventing token reuse after sign-out.
- **Full-system backup**: admin-only `GET /api/admin/backup` streams the entire database as NDJSON; `POST /api/admin/restore` replays it with a preview/confirm wizard in the UI.
- **Streaming download**: backup endpoint streams response bytes directly from the database without buffering the full payload in memory.

### Fixed
- Search placeholder no longer shows stale text after clearing the search input.
- Logout correctly terminates the Keycloak SSO session via `end_session_endpoint` redirect.
- JWT `aud` claim made optional; Keycloak audience mapper configured in realm export.
- 401 reload loop: app children are lazily instantiated so a failed token refresh does not trigger an infinite reload cycle.

## [1.7.0] - 2026-03-17

### Added
- **Backup / restore UI**: admin panel with a multi-step preview/confirm wizard for full-system backup and restore.
- **Task sync**: task toggle is propagated across My Day and NodeView via a shared `TaskRefresh` context signal.

### Fixed
- Session cookies cleared with correct path on logout.
- `end_session_endpoint` rewritten with `OIDC_EXTERNAL_URL` so the browser receives a browser-reachable Keycloak URL.
- Post-logout redirect URI added to Keycloak client config.

## [1.6.0] - 2026-03-17

### Added
- **Extended search**: full-text and fuzzy search now covers notes and task text in addition to node titles and bodies.

## [1.5.0] - 2026-03-17

### Added
- Collapsible panels in NodeView.
- Dashboard sidebar item renamed for clarity.

### Fixed
- Notes feed expands to full available width.
- My Day and Dashboard empty states are vertically and horizontally centred.

## [1.4.0] - 2026-03-17

### Added
- **Notes**: per-node append-only timestamped notes with a global feed view (`/api/notes/feed`).

## [1.3.0] - 2026-03-17

### Added
- **Tasks**: per-node task lists with create / toggle / delete / My Day scheduling (`/api/nodes/{id}/tasks`).
- **My Day view**: aggregated view of all tasks scheduled for today with focus-date planning.
- **Project Dashboard**: task counts and status summary for Project-type nodes.
- **Node templates**: pre-filled Markdown templates for each node type (article, project, area, resource, reference).

## [1.2.0] - 2026-03-17

### Added
- Quick-capture FAB: floating amber button (bottom-right) opens a modal for rapid node creation with title, type, and optional notes fields; Ctrl+Enter to save, Esc to cancel; navigates to new NodeDetail on success.

### Changed
- **Ember warm theme**: replaced all cool-gray tones with Tailwind `stone` palette and blue accents with `amber`/`orange`, delivering a warm "winter fire" aesthetic consistent across both light and dark modes.
  - Light mode: `stone-50` parchment background, `stone-900` text, `amber-600` primary actions.
  - Dark mode: `stone-950` near-black background, `stone-100` text, `amber-400` links and accents.
  - Graph edges: References use `amber-600`, WikiLinks use `orange-400`.
  - Keycloak login theme updated to match warm ember palette.

## [1.1.0] - 2026-03-17

### Added
- Keycloak login theme: CSS-only dark theme matching app palette.
- Wiki-link `[[title]]` syntax: auto edge creation, UI autocomplete, click navigation, unresolved strikethrough.
- CI/CD: `.github/workflows/ci.yml` (cargo check/clippy/test + WASM job) and `.github/workflows/release.yml` (cargo-dist cross-platform binaries).
- User management UI + Keycloak admin integration.

## [1.0.0] - 2026-03-17

### Added
- Initial production release.
- All 8 implementation phases complete: workspace skeleton, OIDC auth, Node CRUD + Markdown editor, knowledge graph (edges + tags), full-text/fuzzy search, attachments + S3, per-node permissions, Docker multi-stage + K8s deployment.
