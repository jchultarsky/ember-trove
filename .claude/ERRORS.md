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
