# Ember Trove — Roadmap & Architecture Decisions

Living document: current state, backlog, and the decisions behind the architecture.
Keep it current as part of each change (see `POLICY.md` §10).

## Current state (2026-10-27)

- **v2.42.0 shipped: session renewal in place (#161 gap; PR #176), member
  card contacts (#172; PR #177), Role combobox (#171; PR #179), sidebar vs
  section labels (#173; PR #181), deploy image cleanup (PR #169).**
  - **A 401 renews the session and re-sends the call, no reload.** Every
    call goes through `api::send_authed`; concurrent 401s share one refresh
    (`common::session_retry::RefreshGate`), and an unanswered refresh (10 s)
    is an ordinary error, not a logout.
  - **Release deploys prune unused images and build cache** before the pull
    and after a healthy deploy (the v2.41.0 full-disk failure). Never volumes.

- **v2.41.0 shipped: draft staleness and expiry (#161 gaps; PR #168).**
  - **Restore says when the node changed since.** Edit drafts carry
    `base_fps` (the editor's `own_fps`); the offer warns that restoring
    replaces a change made elsewhere (it stays in History; History keeps
    bodies only). v2.40.0 drafts say the node *may* have changed.
  - **Drafts expire 14 days after their last write** (`DRAFT_TTL_MS`). An
    expired draft is never offered, and a startup sweep removes every
    account's expired drafts; unstamped v2.40.0 drafts get dated, not lost.

- **v2.40.0 shipped: honest saves (#161; PRs #164, #167), Role and Note at
  attach (#162; PRs #163, #165), and migration 039 (#160).**
  - **Node saves can't stall silently.** "Still saving…" at 5 s; at 15 s
    the save is given up on, with Retry, even when the request never
    settles (the 401 refresh path). Unsaved edits are kept as a local draft
    until a save lands, with Restore / Discard on return.
  - **No late overwrite.** Edit-mode saves send `expected_updated_at`;
    `PUT /api/nodes/{id}` answers 412 for a stale version and writes
    nothing (checked after authorization). The editor re-sends on top of
    its own text and otherwise says "changed elsewhere"; it overwrites only
    on an explicit Retry or Save.
  - **Drafts are per user and wiped at logout**, edit and new-node drafts
    alike; the old shared new-node key is deleted unread (it used to leak
    a draft to the next account in the browser).
  - **People:** picking or creating someone from a node's People section
    can set the membership's Role and Note in the same step.
  - **Process lesson:** issue-monitor PRs (#163, #164) and manually proposed
    sessions duplicated each other on #161/#162. A `claude:handled` /
    `state: in-progress` label means the routine already owns the issue —
    check for its PR before starting a session.

- **v2.39.0 shipped: People follow-ups (#154, #155, #156; PRs #157–#159).**
  - **Addresses are a list**, like emails and phones: zero or more, each
    with a label, a multi-line value and a preferred marker. Migration 038
    moves each old address into a one-entry list and drops the
    `address_label` / `address` columns in the same transaction. Old
    backups and pre-upgrade tabs still work (the API keeps the stored list).
  - **People surfaces follow the button language**: icon-only add, remove
    and close in the person editor and the node's People section; the
    `/people` registry uses the full width.
  - **Creating a person from `/people` returns to the list**, with the new
    row highlighted and a toast, so adding several people costs no Backs.

- **v2.38.0 shipped: the person editor and one button language (#152, #153).**
  - **One person editor** for New, Edit and a node's "Create “…”": a
    centred dialog on desktop, a full-screen sheet on phones
    (`ModalPlacement::Sheet`). Cancel discards; Escape discards with Undo.
  - **The three-tier button rule**, recorded in `.claude/rules/leptos.md`:
    icon-only for header, toolbar, row and inline actions; icon + word for
    dialog commits and calls to action; word-only for Cancel, tabs, pills,
    menus and inline links. 12 buttons became icon-only and 17 gained an
    icon.
  - **`refresh-icon-font.sh` now reads a span's content**, so it no longer
    misses glyphs. It used to take `aria-hidden="true"` for the name.

- **v2.37.0 shipped: People (#147; PRs #148–#151) and no more public links.**
  - **Public share links are removed** (owner decision). Migration 036 drops
    `share_tokens`, so the only unauthenticated content route is gone.
  - **People are an entity, not a node type.** There's a registry of persons
    (a vCard subset) and memberships that place a person on a node with a
    role and a note (migration 037, `node_people`).
  - **Privacy:**
    - A person is visible to its owner only, with **no admin bypass**.
    - A node sharee sees a membership's name and role only.
    - Only the node's canonical owner can change its people.
  - **UI:** a `/people` registry, the `/people/:id` card ("Used in", mailto/tel
    actions, a duplicate-email warning on create and save), and a People node
    section (picker-first add, inline create, role and note).
  - Backups carry people. People appear in global search as their own group
    (the search page, the header dropdown and the palette, with ranking node
    titles → people → commands); `SearchResult` is unchanged.
  - **Fixes found along the way:**
    - the icon font's URL is content-hashed (new glyphs used to show as text
      for up to 7 days);
    - an unmount-after-await panic on the search page.
  - **Architecture decision:** documents vs entities. People are the first
    owned entity that isn't anchored to a node, so their authorization is
    owner-based rather than following node permissions. A later ontology
    session can build on that without unpicking it (deferred, owner decision
    2026-10-26; see Backlog).

- **v2.36.0 shipped: a project's status is a field (#143, PR #144).**
  - **The field:** `nodes.project_status`, an open-valued token of at most
    40 characters. It's named that way because `status` is the lifecycle.
  - **Where it shows:** a header chip with a picker that suggests your own
    values and folds case variants together, a Projects-list badge and
    filter, and dashboard badges.
  - **Moving old data:** migration 035 adds the column and a
    `data_backfills` marker. A one-time Rust backfill (`api/src/backfill.rs`)
    moves `## Status` tokens out of project bodies. It snapshots the old body
    as a version first and doesn't bump `updated_at`.
  - **Architecture decision:** this is section promotion again, not a
    property framework. Generalize only once a second real field appears.

- **v2.35.0 shipped: the v2.34.0 code-survey issues (#129–#135; PRs
  #136–#142).**
  - **Deploys reach open tabs.** The service worker serves navigations
    network-first and hashed assets cache-first, and a floating banner
    offers Reload when the bundle is stale. Cached personal data is purged
    on logout and on a 401.
  - **No duplicate connections.** Migration 034 archives existing
    duplicates and adds a unique index; a repeated link now gets a 409 and
    an "already linked" hint.
  - **One component per job.** Every node-choosing field is `NodePicker`.
    Every node-page section is a `SectionShell`, reachable from a
    "Sections" outline. Every task list uses `TaskEditor` and
    `PriorityPicker`, a labelled radio group (not colour-only dots).
  - **Named buttons.** Icon-only buttons now have real accessible names,
    there is one connection-type list, and an e2e test scans for unnamed
    buttons.
  - **Wiki links resolve on demand** through the caller-scoped
    `GET /api/nodes/resolve-titles`; the client no longer downloads every
    title.
  - **More realistic e2e.** A second test identity and a `seedTrove`
    fixture; clippy now runs with `--all-targets`; the coverage floor is
    41%.

- **v2.34.0 shipped: node pickers fixed (#127, #128; issue #126).** Every
  picker that commits a node is one APG combobox (`NodePicker`): text
  filters, only Enter on the active option or a press commits, a chip,
  arrows, scoped Escape, blur, ARIA. Same-titled results need an explicit
  choice, and Enter before results arrive resolves only a unique exact
  title. Connections uses it (no more "Invalid target node UUID"). Pickers
  search **titles** via `GET /api/nodes/lookup` (exact > prefix > other);
  full-text search had matched bodies and whole words only, so "Physna"
  showed body mentions and "Phy" nothing. The palette keeps full-text.

- **v2.33.0 shipped: strict CSP + #120 + #123 (#121, #122, #124).** The
  prod CSP has no `'unsafe-eval'` (only `'wasm-unsafe-eval'`), and e2e fails
  on any CSP violation. The loading screen shows its build-stamped bundle
  version, plus the server's when they differ (a stale-cache signal). On the
  node page: Jump to tasks/notes; the page boundary is a `<Transition>`, so
  adds no longer reset scroll; editor Escape stops propagating (it used to
  navigate to /nodes) and offers Undo for unsaved edits. Issue reviews are
  now posted on the issue ("Review before implementing"). **Next
  candidates:** #101 tower-http (waiting on reqwest).

- **v2.32.1 shipped: the code-review backlog is closed (#115–#119).**
  Review bugfixes (metrics, invite links, refresh renewing the access
  cookie, month-end recurrence, trashed rows in search, no `js_sys::eval`).
  Search highlights only the returned page (1,367 → 61 ms). `wire_enum!`
  gives one wire table per enum. The UI has one task PATCH helper and
  editor draft (`task_actions`), one node-search hook, `ModalShell` for
  every modal and one editor-height store. The e2e fixtures guard every
  spec against page errors. CI runs `cargo test -p ui`, and handler-level
  authZ tests cover the v2.31.1 fixes.

- **v2.32.0 shipped: #111 + AWS SDK update (#114, #113).** Every create
  path resolves a new node's body by one rule
  (`common::template::resolve_create_body`: prefill > draft > type default >
  scaffold). Body swaps happen only while it's untouched (`may_replace_body`).
  Sidebar "+" per type and palette "New <Type>" → `/nodes/new?type=…`.
  "Use as default" in the picker; the picker is grouped by type. AWS SDK
  bumped coherently (one aws-smithy-schema) now that `sigv4a` is gone.

- **v2.31.1 shipped: security hotfix (#112).** Wiki links and
  neighbors/backlinks/edges are scoped to what the owner or caller can see
  (migration 033 archives the removed links); permission revoke is
  node-scoped; there's a canonical-owner guard on all grant/update/revoke
  paths; task re-parenting needs editor on the destination; SSRF judges
  IPv4-in-IPv6 as IPv4; the graph single upsert is owner-scoped; webhook URLs
  aren't logged; `sigv4a` dropped (RUSTSEC-2026-0387). The review
  follow-ups it listed shipped in v2.32.1.

- **v2.31.0 shipped: #98 complete + search results (#105, #106, #109,
  #110).** Due dates read back in words with a past-date warning (one
  `DueDateField` for all five inputs). Export names `{slug}-{updated_at}`.
  The header search clears off /search. "Saved · <time>" (formatted without
  `eval`). Built-in templates carry no prose. Deletes and the row ☀ go
  through `task_actions` (`delete_with_undo`, `toggle_my_day`,
  `MyDayButton`). Search (#107, #108 from jchultarsky101): full titles via
  `HighlightAll` + the `title_highlight_is_full` guard, `<mark>` highlights
  finally render (the sanitizer used to allow only `<b>`), no slug, a
  type/updated/"matched in" meta line, client-side marks for fuzzy and
  note/task results, match-less snippets hidden, a "Recently updated"
  browse label. **Follow-ups:** `format_timestamp` still formats via
  `js_sys::eval` (fine under prod CSP, ISO fallback locally); Dependabot
  #100 (AWS schema skew) and #101 (tower-http dup) are still deferred.

- **v2.30.0 shipped: #98 UI feedback, items 2 + 7 (PRs #103, #104).**
  Completing a task offers Undo everywhere. Carried-over tasks stay in My
  Day as done (the browser re-stamps their My Day date to today). Reopening
  a recurring task retracts its scheduled occurrence (migration 032
  `recurrence_parent_id`). Security: occurrences keep the series owner
  (they used to go to whoever completed the task). All done-toggles go
  through `task_actions::toggle_done`. New-task form has an Add to My Day
  toggle. ctutils RUSTSEC-2026-0385 fixed (0.4.5). utoipa 6 (#102).
  Deferred: the AWS SDK group bump (#100, upstream schema skew + days-old
  crypto) and tower-http 0.7 (#101). **#98:** item 1 (`DueDateField`) and items 3/4/5/6
  (export name, header search clear, saved-at time, template prose) are in
  PRs #105 and the ui-polish PR. **Follow-up refactor:** move
  delete-with-undo (4 copies) and the My Day toggle (2 copies) into
  `task_actions`.

- **v2.29.0 shipped: collapsible notes + audit review (PRs #96, #97).** Each
  note on a node page collapses to its first line (Markdown stripped via
  `common::note::note_title`, so it works as a title), with Collapse all /
  Expand all. State is per account in `editor_prefs` (migration 031:
  `collapsed`, nullable `height`). PUT /editor-prefs changes only the fields
  sent; bulk PUT /editor-prefs/collapsed takes up to 500 ids. cargo-audit
  ignores reviewed: 4 → 2 (stale lru/rand dropped; newer lru fixed by
  aws-sdk-s3 1.152; rsa rationale corrected: sqlx-mysql is lockfile-only),
  and no warnings remain. chultarsky.online fully decommissioned (registration,
  Route 53 zone and the leftover Lightsail DNS zone).

- **v2.28.3 shipped — TLS advisories, Leptos 0.8.22, note delete (PRs #87,
  #89, #92–#95):** rustls-webpki 0.103.16 / webpki-root-certs 1.0.10
  (RUSTSEC-2026-0382/0383, distrusted intermediates on outbound TLS). Leptos
  0.8.22 unblocked by moving the transitive server_fn_macro_default to 0.8.6
  (ERRORS.md documents the syn-3 lockfile trap). Drops the unmaintained
  proc-macro-error2. Notes can now be deleted from a node's page (shared
  delete + undo with the Notes feed). Dependabot cargo + Actions bumps.
  PHP2Shell (CVE-2026-109618/109619) checked: no PHP anywhere in repo, images
  or the prod host. Open item: chultarsky.online A records still dangle at a
  released Lightsail IP — delete in AWS (needs console/creds).

- **v2.28.2 shipped — security deps + build/e2e hardening (PRs #88, #90, #91):**
  ammonia 4.2.4 (RUSTSEC-2026-0373, CSS `url(` sanitizer bypass); direct `zip`
  8 → 9 and the unused `utoipa-swagger-ui` removed (it pulled vulnerable zip
  3.0.0 at build time and was never mounted) — `cargo audit` clean, no new
  ignores. `Dockerfile.ui` installs a pinned, SHA-256-verified trunk 0.21.14
  binary (no more unpinned `curl | bash` binstall + unlocked source fallback).
  e2e flakes fixed: write rate limit relaxed only in armed e2e-bypass builds
  (resolves the v2.28.1 backlog note), and the palette now closes on Escape
  regardless of focus timing.

- **v2.28.1 shipped — Cognito forward-compat + CI unblock (PRs #85, #86):**
  Cognito's additive `acr`/`amr` claims and new discovery members (rollout from
  2026-11-04) need no change — regression tests pin the tolerance. JWKS now
  refetches on an unknown `kid` (key rotation; rate-limited 60 s, shared
  attempt, falls back to cache on failure) and all IdP HTTP calls have a 10 s
  timeout. Lockfile cleared 12 RUSTSEC advisories (ammonia 4.2.3, rustls
  0.23.46, h2, quinn-proto). MinIO's community images were withdrawn, so
  local/e2e/k8s S3 is now SeaweedFS 4.48 (private bucket). Repo now allows
  auto-merge and requires all 11 CI checks on develop+main.
  Backlog: e2e trips the API write rate limiter (429) when run fast locally —
  CI passes; consider an e2e-bypass-gated limiter budget if it reaches CI.

- **v2.28.0 shipped — earth-tone graph palette (PR #73):** approved comp
  (brick-red variant) implemented; ember (Article) is the sole warm anchor,
  Project/Area/Resource/Reference muted to slate-indigo #5c6f96 / moss
  #78864f / plum #8a5a7e / brick #a1493c with darkened strokes; legend and
  minimap follow. Saturated red #dc2626 (heat-max) is now reserved for
  urgency. Edge colors deliberately unchanged — revisit only if the node
  change lands well. As-approved record:
  https://claude.ai/code/artifact/11bff6cb-2a3d-4c29-b084-89d52e89a57c

- **v2.27.2 shipped — backlog sweep (PR #72):** all primary CTA backgrounds
  moved onto the phase-3 heat tokens (`bg-ember`/`hover:bg-ember-strong`,
  new `--color-ember-strong` token, identical hexes — zero visual change);
  the last 25 bare-`rounded` form controls normalized to `rounded-lg`;
  change-password modal joined `rounded-2xl`; POLICY §13 corrected (coverage
  is a hard 24% gate since v2.23.0, not report-only). Remaining amber →
  token migration (dark-variant/text ambers) deferred: needs per-theme
  token values. Graph node-type categorical palette reconciliation awaits
  a design decision (mock first).

- **v2.27.0 shipped — visibly resizable note/task editors (PR #71):**
  `ResizableEditor` gained an always-visible bottom drag bar (pointer
  events, capture, 64px floor; native corner grip kept); the node page's
  note edit field converged on the shared component (its hand-rolled
  textarea duplicated the resize/persist logic); save/cancel dedup'd.
  Per-note heights persist via editor-prefs as before.


- **v2.26.0 shipped — design refresh (three-phase arc, PRs #63/#66/#65):**
  full UI audit → approved interactive comp → three stacked slices.
  Phase 1: fonts self-hosted (Material Symbols subsetted to the ~96 used
  glyphs, 91 KB vs 4 MB, `font-display: block` kills the icon-name FOUT;
  offline PWA icons work for the first time; `scripts/refresh-icon-font.sh`
  regenerates after adding a NEW icon name); one primary `amber-600`; one
  modal backdrop recipe; CSP dropped the Google Fonts origins in all three
  nginx configs; `index.html` ships `Cache-Control: no-cache` via
  `expires -1` (preserves add_header inheritance — the trap is documented
  in-file). Phase 2: `PageHeader` + `EmptyState` primitives (7 views + 5
  settings pages aligned; `.claude/patterns/page-scaffold.rs`); radius
  tiers cards-lg/popovers-xl/modals-2xl. Phase 3: Fraunces display face
  (`.font-display`, titles/wordmark/zone names/empty states only),
  Tailwind v4 `@theme` heat tokens (ember/glow/heat-high/heat-max — warm =
  urgency, `amber-*` migrates incrementally), spark-on-complete
  (`ui/src/spark.rs`, →done only, reduced-motion safe), hearth meter in the
  My Day header. Also: ammonia 4.1.4 (RUSTSEC-2026-0213, published and
  fixed same day). Process lesson: stacked-PR merge closed #64 when the
  base branch was deleted → recreated as #66 (see memory:
  stacked-pr-merge-order).

- **v2.25.0 shipped — unified task rows (three-phase arc, PRs #60–#62):** all
  task lists (My Day, Inbox, node task panel) render through one display-mode
  scaffold (`ui/src/components/task_row_scaffold.rs`): checkbox | title line
  over a wrapping meta line | compact right action cluster in one order
  (context · edit · delete). Title leads everywhere; meta (parent chip, due
  badge, recurrence, status, carryover) sits below and wraps on phones;
  titles wrap below `sm`, truncate at sm+. Carryover became a "carried
  ‹date›" badge with one ✓ (aria "Keep on today") — the "No" button
  duplicated the ✕ remove action and was deleted. Inbox de-carded (flat rows
  in one bordered divide-y list); priority is the colour dot with accessible
  name everywhere (label/icon variants and their helpers deleted); overdue
  colours the due badge, not the title. Design was mockup-approved before
  implementation; each phase verified at 375px + desktop. Also in this
  release window: v2.24.4 (My Day titles wrap on phones — superseded by the
  full arc same-day).


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
- **Zero-AWS local login (PR #43, merged 2026-07-17):** a bundled Keycloak issuer (`./scripts/dev-local.sh`) whose
  `cognito:groups` protocol mapper leaves the token path unchanged; only API
  change is a Cognito-only guard on `/auth/change-password`. Verified
  end-to-end (login → `/api/auth/me` roles:["admin"]). The README quickstart
  documents it; the unstyled login page was fixed later (theme assets served).
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
  produced the plan of record below. Its three security findings shipped in
  v2.23.0 (the planned v2.22.4 patch was folded into it).

## Plan of record (2026-07-17 review) — closed

Every item of that plan has shipped or been re-filed; details are in the
release entries above and in git history.

- **v2.22.4 security patch:** folded into v2.23.0 (rate-limited
  `/share/{token}`, node-scoped token revoke, webhook DNS re-vet + pinning).
  The share-token surface itself was removed in v2.37.0.
- **v2.23.0 "trust the suite":** shipped (route-group tests, graph e2e,
  Postgres repo tests, coverage gate). Webhooks UI kept; `/search` kept.
- **v2.24.0 keyboard & a11y:** phases 0–3 shipped (PRs #47–#50: one
  editable-guard, the `common::keyboard` registry driving dispatch and help,
  overlay suppression, a focusable graph). `KeyboardScope` was dropped as
  unnecessary. Phase 4 (a11y sweep) continued piecemeal: named icon
  buttons and the unnamed-button e2e scan (v2.35.0), APG comboboxes
  (v2.34.0).
- **v3 / OSS launch:** zero-AWS local login shipped in PR #43 (Keycloak,
  `./scripts/dev-local.sh`). Anything beyond it is deferred; see Backlog.
- **Opportunistic refactors** (do while touching the area, never big-bang).
  Done: one task-row scaffold (v2.25.0), the duplicate `nodes(owner_id)`
  index dropped (migration 039). Still open (counted 2026-10-26):
  - merge the three `repo/search.rs` query builders (3
    `too_many_arguments` allows);
  - adopt `#[from] sqlx::Error` in repos (203 `Internal(format!)` sites,
    up from ~146);
  - split `graph_view.rs` (2,171 lines; move pure layout out) and
    `routes/nodes.rs` (1,309 lines);
  - route node export through the UI API client (three raw `<a href>`
    links in `node_list.rs` / `node_view.rs`);
  - a shared debounce helper (the pattern is still re-derived per view).

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
  19 specs today; grow them alongside new UI surfaces, and keep pushing pure
  logic into `common/` for unit coverage.
- **Known gaps after #161 (not yet scheduled):**
  - ~~On a 401 the API client reloads the page instead of retrying~~:
    fixed in v2.42.0 (`api::send_authed`, one shared refresh, retry in
    place).
  - ~~Restore offer silent about a node changed since the draft; drafts
    never expire~~: fixed in v2.41.0.
- Optional: `lldb-dap` for editor step-debugging (not installed; editor-local tooling,
  not a repo deliverable).
- **Deferred by owner decision (2026-10-26), not critical:**
  - **Further local-auth work.** Zero-AWS login already ships (Keycloak via
    `./scripts/dev-local.sh`, PR #43). Anything more, such as running e2e
    against Keycloak instead of the bypass, waits.
  - **The ontology session** (typed relations / entities beyond People).
    People's owner-based authorization model is the starting point when it
    resumes.
- **Blocked upstream:** tower-http 0.7 (#101) waits for reqwest to move off
  0.6, or the bump duplicates the crate.
- **Kept deferred:** block references / transclusion; arrow-key spatial
  graph navigation (keyboard "3b").

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
