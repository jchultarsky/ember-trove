# Leptos / UI Rules (auto-relevant for `ui/`)

Leptos 0.8 CSR/WASM + Tailwind v4 + `leptos_router` 0.8. Canonical code in
`.claude/patterns/`. Build/lint UI with the wasm target:

```
cargo check  -p ui --target wasm32-unknown-unknown
cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings
```

## Navigation (`leptos_router` 0.8)

- Browser back/forward works natively.
- **`NavigateFn` is `Clone`, not `Copy`.** In reactive contexts wrap it in
  `StoredValue` and call via `get_value()`:
  ```rust
  let navigate = StoredValue::new(use_navigate());          // ui/src/components/tasks_view.rs
  navigate.get_value()("/path", Default::default());
  ```
  Or clone it before each inner `move ||` closure. See `.claude/patterns/navigate-reactive.rs`.
- Route paths require the `path!()` macro:
  ```rust
  use leptos_router::path;
  <Route path=path!("/tasks/inbox") view=|| view!{ <TasksView active=TasksTab::Inbox/> } />
  ```
- **URL map:** `/tasks/my-day` · `/tasks/inbox` · `/tasks/calendar` · `/dashboard` ·
  `/graph` · `/search` · `/notes` · `/nodes` · `/nodes/new` · `/nodes/:id` ·
  `/nodes/:id/edit` · `/tags` · `/templates` · `/admin/users` · `/admin/permissions` ·
  `/admin/backup`. Legacy `/my-day`, `/inbox`, `/calendar` redirect to `/tasks/...`
  (bookmarks/PWA shortcuts predating v2.3.0).

## Reactivity gotchas

- **`window_event_listener` handles MUST be removed in `on_cleanup`** — Drop
  does not detach; a leaked listener panics on disposed signals and poisons
  all event dispatch (see ERRORS.md, 2026-06-10).
- **No `use_context` after an `.await`** in `wasm_bindgen_futures::spawn_local`
  — there is no reactive owner there; capture context values before spawning
  (toast.rs has a global fallback for this reason).

- **Static `style=` / `title=` must be closures** — `style=move || ...` — for reactive attrs.
- **Moving a non-`Copy` value into an inner closure makes it `FnOnce` and breaks
  reactivity.** Clone signals/`navigate` before the inner `move ||` in each `map` iteration.
  See `.claude/ERRORS.md`.
- **Shared submit logic:** use an `RwSignal<bool>` trigger + `Effect::new`; any handler
  sets it `true`, the effect does the work once and resets it. Real example:
  `ui/src/components/modals/create_node.rs` (`submit_pending`). See
  `.claude/patterns/submit-trigger.rs`.
- **Debounced search:** version counter + 300 ms `Timeout`; only the latest version
  commits. Real example: `ui/src/components/notes_view.rs` (`debounce_v`). See
  `.claude/patterns/reactive-effect-debounce.rs`.
- **Context newtypes** to prevent collisions:
  `#[derive(Clone, Copy)] struct ShowCapture(pub RwSignal<bool>);` (see `ui/src/app.rs`).
- **Required context = `expect_context::<T>()`, never `use_context::<T>().expect(..)`.**
  A missing provider is a build-time wiring bug, not a runtime/data failure, so the
  blessed Leptos helper is the sanctioned exception to the zero-panic lint
  (`clippy::expect_used`) — it reads as intent and keeps the lint strict for genuine
  `.unwrap()`/`.expect()` on fallible values. For a context that may legitimately be
  absent, use `use_context::<T>().unwrap_or_else(|| …)` instead (real example:
  `ui/src/components/node_list.rs`, `tag_filter`).

## SVG & Tailwind

- SVG z-order = DOM order. Use `style="..."` for hyphenated attrs (`stroke-width`,
  `stroke-linecap`, …), **not** `attr:`.
- Tailwind v4 `group-hover` is unreliable — use an always-visible muted element plus a
  `:hover` color change instead.

## Domain field access

- `MyDayTask` fields are reached through the nested task (serde `flatten`):
  `my_day_task.task.node_id`.
