# Known Errors & Fixes — self-learning log

Grep this **before** debugging a compile/runtime/CI error or inventing a new pattern.
Append new entries as you learn them. Format: **Symptom → Cause → Fix → date**.

---

## Rust / Cargo

### `cargo fmt` surfaces new clippy lints (e.g. `unnecessary_sort_by`)
- **Symptom:** clippy was green, you ran `cargo fmt`, and now `clippy -D warnings` fails
  (e.g. `unnecessary_sort_by` on `nodes.sort_by(|a, b| a.x.cmp(&b.x))`).
- **Cause:** rustfmt rewrites a block-body closure `|a, b| { expr }` into an
  expression-body closure `|a, b| expr`; some clippy lints only match the latter. fmt
  didn't break the code — it exposed a latent lint.
- **Fix:** always run clippy **after** fmt (the pre-commit hook does fmt+clippy in order).
  For this lint, prefer `sort_by_key(|n| key)` / `sort_by_key(|n| Reverse(key))`.
  Real fix: `ui/src/components/node_list.rs::sort_nodes`. — 2026-06-05

### AWS SDK pulls advisory-laden rustls 0.21 chain
- **Symptom:** `cargo audit` flags RUSTSEC-2026-0049/0098/0099/0104 (rustls 0.21 /
  rustls-webpki 0.101.7).
- **Cause:** AWS SDK crates' default/legacy `rustls` feature.
- **Fix:** set `default-features = false` and use `default-https-client` (rustls 0.23 +
  aws-lc-rs) on `aws-sdk-{s3,cognitoidentityprovider,sesv2}`. Verify with
  `cargo tree -i rustls-webpki@0.101.7` (must return nothing) + a live TLS round-trip.
  — 2026-05-29

### `cargo audit` ignore list diverged from CI
- **Symptom:** an advisory ignored locally still fails in CI (or vice-versa).
- **Cause:** inline `--ignore` flags in `ci.yml` *and* a list in `.cargo/audit.toml`.
- **Fix:** `.cargo/audit.toml` is the **single source of truth**; CI runs bare
  `cargo audit`. Never add inline `--ignore`. Each ignore is transitive-only and dated. — 2026-05-29

### e2e/dev stack: `pull access denied for minio/minio`
- **Symptom:** `e2e smoke` fails in ~6 s at `docker compose up` with
  `pull access denied for minio/minio, repository does not exist` (also `minio/mc`).
- **Cause:** MinIO withdrew its community images from Docker Hub and quay
  (2026); there is no tag to pin back to.
- **Fix:** local/e2e/k8s S3 is SeaweedFS (`chrislusf/seaweedfs`, pinned tag) —
  service `s3` on :9000, admin identity from `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`,
  `-ip.bind=0.0.0.0` (else in-container healthchecks get "connection refused"),
  private bucket created by `s3-init` via `weed shell` (`s3.bucket.create`; it
  prints "already exists" with exit 0 on re-run, so grep the output). Pin
  third-party images to exact tags so a registry change fails loudly in one place.
  (2026-10-16)

### e2e: random specs fail with `"Too Many R"... is not valid JSON` / fixture `resp.ok()` false
- **Symptom:** a different handful of Playwright specs fails each run (graph
  fixtures, toasts, search presets); one surfaces `SyntaxError: Unexpected token
  'T', "Too Many R"...`. More frequent on fast machines; intermittent in CI.
- **Cause:** the API's write rate limiter (10 req/s, burst 100 per IP) — the
  whole suite shares one client IP and creates fixtures via the API.
- **Fix:** `rate_limit_budget` in `api/src/routes/mod.rs` relaxes the budget only
  when compiled with `e2e-bypass` AND armed with `E2E_AUTH_BYPASS=1` (release
  images can't select it). Don't "fix" this by retrying 429s in specs — that
  hides real limiter regressions. (2026-10-16)

### UI image build: `CSSString<'i>: cssparser::serializer::ToCss` not satisfied (lightningcss)
- **Symptom:** `docker build -f deploy/Dockerfile.ui` (e2e job or release) fails
  compiling trunk; log shows `cargo-binstall: WARN Timeout reached while checking
  fetcher` then `will be installed from source`.
- **Cause:** binstall's download timed out and it fell back to `cargo install
  trunk` WITHOUT `--locked`, pulling a newer `cssparser` that `lightningcss`
  alpha doesn't build against.
- **Fix:** the Dockerfile now curls a pinned trunk release binary and checks
  its SHA-256 (no binstall, no source fallback). Bumping trunk = update
  `TRUNK_VERSION` + both checksums from the release's `.sha256` assets, verified
  against the tarballs. Any `cargo install` of a binary tool: use `--locked`.
  (2026-10-16)

### UI build: `the trait Parse is not implemented for syn::Path` (Leptos `server_fn_macro`)
- **Symptom:** a Dependabot cargo group bumping Leptos (e.g. #87:
  `leptos_router` 0.8.15 → 0.8.17) fails `check (wasm32)`, `docker build
  validation` and `e2e smoke` with `server_fn_macro_default-0.8.5/src/lib.rs:
  ... Parse is not implemented for syn::path::Path` and "multiple different
  versions of crate `syn`" (2.x vs 3.x).
- **Cause:** a partial lockfile update. `leptos_router` 0.8.17 → `leptos`
  ^0.8.22 → `leptos_macro` =0.8.19 → `server_fn_macro` =0.8.12, which moved to
  **syn 3**. `server_fn_macro_default` 0.8.5 (syn 2) declares the loose
  `server_fn_macro ^0.8.5`, so Cargo keeps it and pairs it with 0.8.12. The
  matching `server_fn_macro_default` **0.8.6** (syn 3, `=0.8.12`) exists, but it
  is transitive (`server_fn` asks `^0.8.5`), so Dependabot never bumps it.
- **Fix:** move it with the rest of the family:
  `cargo update -p leptos_router --precise <ver> && cargo update -p
  server_fn_macro_default`. Verified 2026-10-22: leptos 0.8.22 /
  leptos_router 0.8.17 / server_fn_macro 0.8.12 / server_fn_macro_default 0.8.6
  builds for wasm32 (and drops the unmaintained `proc-macro-error2`). Don't
  "fix" it by holding Leptos back. #87 did that on 2026-10-18 (Leptos kept at
  0.8.20 / 0.8.15) before the real cause was found. Pinning individual
  `--precise` versions down the chain is whack-a-mole. (2026-10-22)

## Axum / API (HTTP)

### Browser-navigation endpoint renders JSON or a blank page
- **Symptom:** after an OAuth round-trip the user sees a literal `{"error":...}` as the
  page, or a **blank page**. `/api/auth/callback` is the usual culprit.
- **Cause:** endpoints the *browser navigates to* (OAuth callback, any `Location:` target,
  server-rendered HTML) are not XHR. Two failure modes have shipped: (1) the shared
  `IntoResponse for ApiError` emits JSON — correct for XHR, but the browser renders it as
  page text (≤ v2.x). (2) an HTML response that drives the redirect with an **inline
  `<script>`** is blocked by the strict CSP (nonce + `strict-dynamic`) → blank page; this
  broke login in **v2.20.0**.
- **Fix:** navigation endpoints must respond with a real **3xx redirect** (`Redirect` / 303)
  — never JSON, never an inline-script-driven HTML page. Wrap the handler body in an inner
  `Result` helper and at the outer layer convert any error to `Redirect::to(frontend_url)`
  (log the cause at warn-level). Do **not** `?`-propagate `ApiError` from these handlers; a
  missing PKCE verifier / expired session is a normal redirect-to-frontend case, not a 500.
  — 2026-04-27 / 2026-06-06

### `assert_route_registered` passes for routes that don't exist
- **Symptom:** a route-registration test in `api/src/tests.rs` passes even though the
  route was never added to the router (TDD "red" phase silently green).
- **Cause:** `require_auth` is applied with `.layer()`, and Axum `layer()` middleware
  runs **before route matching** — every path under the protected tree gets the
  500 "OIDC not configured" short-circuit, never a 404. So the `!= NOT_FOUND`
  assertion is vacuous for any nested path.
- **Fix:** drive the real handler instead: build a bare router slice with claims
  injected — `Router::new().nest("/tasks", task_router()).layer(Extension(fake_claims())).with_state(test_state())`
  — and make the relevant stub repo return canned data so you can assert on the
  handler's response. Real example: `task_restore_returns_restored_task_for_owner`
  in `api/src/tests.rs`. (2026-06-10)

### Zombie `window_event_listener` panics after view unmount
- **Symptom:** WASM `RuntimeError: unreachable` in the console on a keypress,
  stack through `window_event_listener::{{closure}}` → `get_untracked`; after the
  first panic ALL event handling silently dies (poisoned dispatch). In prod
  v2.21.0 this killed quick-capture and toasts after leaving My Day.
- **Cause:** `window_event_listener` returns a handle whose **Drop does NOT
  detach the listener**. Discarding the handle leaks the listener past the
  view's lifetime; its closure then reads disposed signals and panics.
- **Fix:** always `let handle = window_event_listener(...); on_cleanup(move ||
  handle.remove());` (layout.rs is the canonical example; my_day_view.rs was
  the offender — its comment even claimed auto-cleanup). (2026-06-10)

### Toasts silently dropped from `spawn_local` continuations
- **Symptom:** `push_toast`/`push_undo_toast` after an API call shows nothing —
  no error, no toast. v2.21.0 undo toasts never rendered in production.
- **Cause:** code resumed after an `.await` inside
  `wasm_bindgen_futures::spawn_local` has **no reactive owner**, so
  `use_context::<ToastState>()` returns `None` and the push is a no-op.
- **Fix:** `toast.rs` keeps a `thread_local` global handle set in
  `ToastState::new()`; the free helpers fall back to it when context is absent.
  General rule: never rely on `use_context` after an `.await` — capture
  context values before spawning. (2026-06-10)

## CI / tooling

### `cargo fmt --check` fails across the whole repo
- **Symptom:** the new fmt gate fails on hundreds of pre-existing lines.
- **Cause:** the codebase was hand-formatted and never run through default rustfmt.
- **Fix:** one-time `cargo fmt --all` pass (committed as `style:` in isolation), then the
  gate is greenable. Editors must format with `--edition 2024` to match the workspace. — 2026-06-05

### `clippy::items-after-test-module` fails though `cargo check` is clean
- **Symptom:** `cargo check` is green, but `cargo clippy --tests -- -D warnings` fails on a
  file that has an inline `#[cfg(test)] mod tests`.
- **Cause:** any non-test item (a free `fn`, a `static`, a re-export) placed **after** the
  inline test module trips the lint. Editing elsewhere in the file can surface a latent,
  pre-existing violation.
- **Fix:** keep `#[cfg(test)] mod tests { … }` the **last** item in the file; never add pub
  items after it. If you find a pre-existing violation while editing the file, fix it in the
  same change — CI fails as soon as anything re-checks that file. — 2026-04 (Phase 1)

### CLAUDE.md referenced non-existent files
- **Symptom:** "grep `.claude/ERRORS.md` / `.claude/patterns/` before debugging" — but
  the `.claude/` tree didn't exist; every pointer was dead.
- **Cause:** docs described an intended structure that was never created.
- **Fix:** created the `.claude/` tree (this file, `POLICY.md`, `ROADMAP.md`, `rules/`,
  `patterns/`) so the references resolve. Keep docs and filesystem in sync. — 2026-06-05

### Resolving "both sides appended here" conflicts loses shared lines
- **Symptom:** after merging `develop` into a PR branch, a spec fails to
  parse ("Unexpected token" at EOF), or Rust tests won't compile.
- **Cause:** when two branches append new tests at the same spot, git puts
  the *shared* closing lines (`  }\n});`, the `.unwrap();` tail of an
  `oneshot(...)` chain) outside the conflict markers. Joining "ours + theirs"
  keeps only one copy, so one test loses its ending, or git interleaves the
  two tests mid-body.
- **Fix:** don't blindly concatenate. For code, take one side whole (`git
  checkout --theirs <file>`) and re-append this branch's addition from `git
  show <branch>:<file>`; then run the file's tests (e2e parse, `cargo test`)
  before pushing. Plain-prose files (CHANGELOG, rules) are safe to join. —
  2026-10-25

## Leptos / WASM

### Reactivity silently stops working
- **Symptom:** a closure compiles but the view never updates; clippy/compiler may warn the
  closure is `FnOnce`.
- **Cause:** a non-`Copy` value (signal, `NavigateFn`) moved into an inner `move ||`,
  consuming it so the closure can only run once.
- **Fix:** clone the value before each inner closure, or wrap in `StoredValue` and use
  `get_value()`. See `.claude/patterns/navigate-reactive.rs`. — (carried from CLAUDE.md)

### `NavigateFn` won't go into a reactive context
- **Symptom:** `use_navigate()` result errors when captured by multiple closures.
- **Cause:** `NavigateFn` is `Clone`, not `Copy`.
- **Fix:** `let navigate = StoredValue::new(use_navigate());` then
  `navigate.get_value()("/path", Default::default())`. — (carried from CLAUDE.md)

### Overlay ignores Escape intermittently (palette e2e flake)
- **Symptom:** `palette.spec.ts` › "…closes on Esc" times out on
  `expect(input).not.toBeVisible()` — the palette stays open after
  `keyboard.press('Escape')`; the immediate re-run passes.
- **Cause:** Escape was handled only by the panel's own `on:keydown`, which needs
  focus *inside* the panel, while the input was focused in a
  `requestAnimationFrame` callback after open. Playwright's `toBeVisible()` checks
  layout, not paint, so it can pass — and the Escape fire — before that frame
  (rAF is throttled in loaded/headless runners). The key then reached the window
  dispatcher, whose Escape arm knew only help + node-page back-nav.
- **Fix:** every exclusive overlay must be closable from the window dispatcher
  regardless of focus (`common::keyboard::escape_target`, palette first); the
  panel handler `stop_propagation()`s so the two never both run (otherwise the
  now-closed palette lets Escape fall through to "back" on `/nodes/*`). Focus on
  mount by tracking the `NodeRef` in an `Effect`, not via rAF. Specs assert
  `toBeFocused()` before relying on focus-dependent keys. — 2026-10-17

### Picker "doesn't filter" and can't select (green tests, broken on real data)
- **Symptom:** in production, typing "Physna" in the Connections picker
  left an unchanging list of unrelated nodes, and the node titled "Physna"
  couldn't be picked. Locally and in e2e, everything passed.
- **Cause:** pickers used the full-text search (body + whole words, sorted
  by title, 8 rows). The fixtures had empty bodies and unique words, so the
  full-text results looked like title matches. Real nodes mention each
  other's names, so the 8 rows were all body mentions, and partial words
  ("Phy") return nothing from full-text search.
- **Fix:** `GET /nodes/lookup` matches titles (substring, exact > prefix).
  The regression test seeds nodes whose **bodies** mention the word and
  types a partial word key by key (`node_picker.spec.ts`).
- **Lesson:** fixtures must look like real data where the feature depends on
  it. A search test needs body text that competes with the title. —
  2026-10-25

### Focusing a button right after the change that enables it does nothing
- **Symptom:** after the picker commits, `confirm.focus()` silently doesn't
  move focus; e2e `toBeFocused()` fails with "inactive".
- **Cause:** the commit sets the signal that drives `disabled`, but the DOM
  updates later in the tick. At `focus()` time the button is still
  disabled, and a disabled button refuses focus.
- **Fix:** set a flag, and focus from an `Effect` that sees the enabled
  state, via `set_timeout(…, 0)` after the render (`EdgePanel`,
  `focus_confirm`). — 2026-10-25

### Page jumps to the top when a panel refreshes
- **Symptom:** adding a task or note on a long node page scrolls back to the
  top. A DOM trace shows the scroll container at `scrollHeight 0` for a frame.
- **Cause:** one `<Suspense>` wrapped the whole node page. A Suspense falls
  back whenever *any* resource read inside it reloads, so a panel's refetch
  unmounted the page and the browser reset `scrollTop`.
- **Fix:** page-level boundaries are `<Transition>`, which shows the fallback
  on first load only (`node_view.rs`). Regression test: `node_page.spec.ts`
  tags the scroll container and checks it survives the add. — 2026-10-25

### Escape in an inline editor navigates away
- **Symptom:** Escape in a node page's task or note editor closes it and also
  goes back to `/nodes`.
- **Cause:** the editor's `on:keydown` handled Escape without
  `stop_propagation()`. By the time the window dispatcher ran, the editor had
  unmounted, focus was on `<body>`, and Escape on a node page meant "back".
- **Fix:** `ResizableEditor` stops the Escape it handles. Every in-panel
  Escape handler must do this (leptos.md). — 2026-10-25

### Shortcut key typed into the overlay it opened
- **Symptom:** pressing `n` opens Quick capture with "n" already in the box.
- **Cause:** autofocus moved from `requestAnimationFrame` to an `Effect` that
  tracks the `NodeRef` (the leptos.md rule). The effect runs in a microtask,
  before the keydown's default action, so the character lands in the newly
  focused textarea. rAF had masked it by focusing after the key was done.
- **Fix:** the dispatcher calls `prevent_default()` on every shortcut it
  handles (`layout.rs`): a consumed key must not also type. e2e:
  `modals.spec.ts`. — 2026-10-24

### Graph auto-arrange spinner hangs forever
- **Symptom:** clicking auto-arrange in the graph spins indefinitely — no PUT fires, no error
  toast, spinner never clears.
- **Cause:** `graph_view.rs::build_adjacency` puts neighbour IDs in adjacency *values* even
  when they aren't *keys*, so BFS over a component can return UUIDs outside it. Downstream
  HashMap indexing keyed only by in-component IDs (e.g. `deg[a]` in `place_component`'s
  `sort_by`) panics with `Option::expect_failed`, killing the `spawn_local` future silently.
  `/api/edges` is unpaginated while `/api/nodes` is (per_page=50), so any edge to an off-page
  or deleted node injects an orphan UUID. Latent since the layout was written; hit in v2.10.5.
- **Fix:** at the top of `smart_layout`, build `id_set: HashSet<Uuid>` from `node_ids` and
  filter `edge_pairs` to edges with **both** endpoints in `id_set`, *before*
  `find_components` / `compute_in_degree` / `place_component`. Treat any node-id-keyed
  HashMap indexing in the layout layer as a panic risk; prefer `.get().copied().unwrap_or(d)`.
  — 2026 (v2.10.5)

### Multi-line code blocks render as a stack of per-line boxes
- **Symptom:** a fenced code block renders each line in its own bordered/background box
  instead of one clean block (worse in dark mode).
- **Cause:** pulldown-cmark renders fences as `<pre><code>`; `<code>` is inline, so any
  `border`/`padding`/`background` on `.prose code` wraps each visual line. Dark mode also
  fights specificity: `.dark .prose code` (0,0,2,1) beats `.prose pre code` (0,0,1,2).
- **Fix:** whenever you touch inline `.prose code` styling in `ui/input.css`, reset every
  property (background, border, padding, shadow) on **both** `.prose pre code` **and**
  `.dark .prose pre code` in the same edit. The dark reset must be on `.dark .prose pre code`
  specifically to win specificity. — 2026-04 (v2.10.2–v2.10.4)
