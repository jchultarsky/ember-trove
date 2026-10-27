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
  `/graph` · `/search` · `/notes` · `/people` · `/people/:id` · `/nodes` · `/nodes/new` · `/nodes/:id` ·
  `/nodes/:id/edit` · `/tags` · `/templates` · `/admin/users` · `/admin/permissions` ·
  `/admin/backup`. Legacy `/my-day`, `/inbox`, `/calendar` redirect to `/tasks/...`
  (bookmarks/PWA shortcuts predating v2.3.0).

## Reactivity gotchas

- **`window_event_listener` handles MUST be removed in `on_cleanup`** — Drop
  does not detach; a leaked listener panics on disposed signals and poisons
  all event dispatch (see ERRORS.md, 2026-06-10).
- **Overlay close keys must not depend on focus.** An exclusive overlay's Escape
  is resolved by the window dispatcher (`common::keyboard::escape_target`); an
  in-panel handler may also handle it but must `stop_propagation()`. Autofocus by
  tracking the `NodeRef` in an `Effect`, not `requestAnimationFrame` (ERRORS.md,
  2026-10-17 palette flake).
- **Page-level data boundaries are `<Transition>`, not `<Suspense>`.** A Suspense
  falls back whenever any resource inside it reloads, which unmounts the page
  and resets scroll (#123, ERRORS.md). Keep `<Suspense>` for small leaf
  sections. Any component that handles Escape itself (editors, pickers) must
  `stop_propagation()`, or the window dispatcher acts on it too.
- **No `use_context` after an `.await`** in `wasm_bindgen_futures::spawn_local`
  — there is no reactive owner there; capture context values before spawning
  (toast.rs has a global fallback for this reason).

- **Forgotten timers and post-`.await` code use `try_` signal access**
  (`try_get_untracked`, `try_set`, `StoredValue::try_update_value`). A
  `Timeout::forget()` or a response can arrive after the component is gone,
  and a plain `get_untracked` on a disposed signal panics. Real example: the
  save watchdog in `node_editor.rs` (#161).
- **A watchdog must not wait for the request it watches** (#161 follow-up).
  `api::handle_error_response` parks a 401'd call forever (refresh, then
  reload), so aborting it does nothing. Give-up timers set the failure state
  and release the lock themselves, and late responses act only through
  `common::save_watch::SaveWatch`. Any path that reloads or redirects must
  assume in-memory edits are lost: keep a draft first.
- **Editor drafts live under `common::save_watch` prefixes** (#161 gaps).
  A new kind of local draft uses a key `is_draft_key` recognises, so logout
  wipes it and the startup sweep expires it (`DRAFT_TTL_MS`, 14 days), and
  it carries `saved_at`, stamped by `crate::draft` on write. Edit drafts also
  carry `base_fps` (the editor's `own_fps`) so the restore offer can say
  whether the node changed since (`EditDraft::recovery`).
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
- **Design tokens & display face (phase 3).** Heat colors live in `@theme`
  (`input.css`): `ember` (the one primary), `glow`, `heat-high`, `heat-max` —
  use the generated utilities (`text-heat-max`, …) in new code; warm color
  means urgency, informational stays stone. Display type is the
  `.font-display` utility (Fraunces + SOFT axis) — page titles, wordmark,
  zone names, empty states ONLY; interface text stays Inter. Completing a
  task must call `crate::spark::strike_spark(&ev)` on the →done transition
  (already wired in all three row components).
- **Node-page sections are a primitive too** (#131): every collapsible
  section under a node body is a `SectionShell` (`node_section.rs`) with a
  `NodeSection` variant. That gives one heading style, a region, the toggle,
  a count badge and an actions slot, and it answers the outline's jumps. A
  new section adds a variant and wraps its body; it never hand-rolls a
  chevron header.
- **Page headers & empty states are primitives, not markup.** Top-level views
  use `PageHeader` (`ui/src/components/page_header.rs`) and `EmptyState`
  (`empty_state.rs`) — never a hand-rolled `<h1>` bar or centered-icon block.
  Radius convention: cards `rounded-lg` · popovers `rounded-xl` · modals
  `rounded-2xl`; card row dividers `divide-stone-100 dark:divide-stone-800`.
  See `.claude/patterns/page-scaffold.rs`.
- **Buttons: icon, icon + word, or word (owner rule, 2026-10-26).** One design
  language everywhere:
  1. **Icon only** for actions in page headers, toolbars, rows, cards and
     inline editors: edit, delete, add, inline save ✓ / cancel ✕, copy, link,
     expand/collapse, close. Use `IconButton` (`icon_button.rs`): its `label`
     is the tooltip and the accessible name. Never a text "Edit" next to an
     icon trash can.
  2. **Icon + word** for the commit button of a dialog or form, and for
     call-to-action buttons (Reload, Logout, Load more). The create-node
     dialog's "+ Create" is the model: a lone ✓ next to ✕ is easy to misread
     at the moment of decision.
  3. **Word only** for a dialog's secondary Cancel, segmented tabs and filter
     pills, menu items, and links inside sentences or notices ("Show 3
     more…", "Save anyway", Undo).

  A new glyph renders as its raw name until it's in the font subset: run
  `scripts/refresh-icon-font.sh` after adding one.
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
