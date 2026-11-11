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
  <Route path=path!("/tasks/captures") view=|| view!{ <TasksView active=TasksTab::Captures/> } />
  ```
- **URL map:** `/tasks/my-day` · `/tasks/captures` · `/tasks/calendar` · `/dashboard` ·
  `/graph` · `/search` · `/notes` · `/people` · `/people/:id` · `/nodes` · `/nodes/new` · `/nodes/:id` ·
  `/nodes/:id/edit` · `/tags` · `/templates` · `/data-health` · `/webhooks` · `/admin/users` · `/admin/permissions` ·
  `/admin/backup` · `/admin/performance`. Legacy `/my-day`, `/inbox`, `/calendar` redirect to `/tasks/...`
  (bookmarks/PWA shortcuts predating v2.3.0). The Inbox became Captures (S4c,
  #240): `/tasks/inbox` and `/inbox` go to `/tasks/captures` through
  `GoTo keep_query=true`, because installed service workers still land a
  share on `/tasks/inbox?captured=1`. `/admin` goes to `/admin/users`
  and unknown URLs to My Day, both through `GoTo` (layout.rs): **never put a
  `<Redirect>` in the `<Routes>` fallback** — it panics (#246, ERRORS.md).

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
  A call whose 401 ends in the login redirect parks forever
  (`api::send_authed`), so aborting it does nothing. Give-up timers set the failure state
  and release the lock themselves, and late responses act only through
  `common::save_watch::SaveWatch`. Any path that reloads or redirects must
  assume in-memory edits are lost: keep a draft first.
- **Every API call goes through `api::send_authed`** (#161 gap). On a 401
  it renews the session once, shared by all calls that got a 401 meanwhile
  (`common::session_retry::RefreshGate`), and re-sends the call in place:
  no page reload. A new request builder belongs in `api/`, takes a `build`
  closure (it runs again for the retry; keep the abort signal on it), and
  never calls `.send()` itself. Only `/auth/me`, `/auth/refresh`, logout,
  `/health` and change-password stay outside, on purpose.
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
- **The Brief is rendered by `BriefBody`** (`node_brief.rs`, #282) on the
  Project page and on the dashboard card (`compact`). On the Project page
  it leads, above the body (`SectionShell`'s `lead`; owner decision,
  2026-11-11): don't move it under the body. A new place that
  shows a Brief uses it, so the age and the stale marker are never left
  off. A write that changes two things (the Brief, then the body, for
  "Move Status section into Brief") saves the copy first and removes the
  original second: a failure in between leaves the words in both places,
  never in neither.
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
  `scripts/refresh-icon-font.sh` after adding one, and commit the font,
  the `?v=` stamps and `ui/icon-names.txt` together. An icon name returned
  by a helper must come from a function whose name contains `icon`, so
  the script finds it (ERRORS.md, 2026-11-10).
- **Phone width is a gate** (#262): `e2e/tests/mobile_layout.spec.ts`
  loads the main pages at 375px and fails on sideways scroll. A header row
  that holds a title and controls needs `flex-wrap`, and the title a
  minimum width, or the title is squeezed to a few characters a line.
- **A paged list is filtered and sorted by the server** (#257). The node
  list sends its kind, filters and `NodeSort` in `NodeListQuery`
  (`api::fetch_node_list`) and shows the rows as they come. Filtering or
  sorting a page in the browser orders only what happened to be loaded.
- **Wiki links are read by `common::wikilink` only** (#281, 3.1). The
  renderer (`wikilink.rs::preprocess_wikilinks`) and the excerpts use
  `replace_wikilinks`; don't scan for `[[` in the UI. The editor's `[[`
  autocomplete is the exception: it looks backwards from the cursor.
- **Rendered Markdown never keeps a class the writer chose** (2026-11-10).
  `markdown::sanitizer` allows `wikilink` on `<a>` and
  `wikilink-unresolved` on `<span>` through `add_allowed_classes`, and
  nothing else: the app's own stylesheet has classes that cover the
  screen. A renderer that needs a new class adds it there, by name.
- **The service worker never writes** (2026-11-10). Any site can make a
  browser post to an address the worker answers, and the worker's own
  requests carry the session. A share goes to the app as
  `/?capture=1#shared=…`; `layout.rs` reads it into the capture box and
  the person presses Capture. Anything else that arrives from outside
  the app gets the same treatment: shown, then confirmed.
- **Data that outlives a session is keyed or cleared**: logout calls
  `draft::clear_all_drafts` and `draft::clear_device_history`;
  `draft::note_signed_in_subject` clears the history when the person
  changes. A new localStorage key that holds a person's content joins one
  of them. `sw.js` purges cached API answers on a 401 from `/auth/me` and
  on a change of person, and never caches `/api/admin/*`.
- **Accessibility basics** (#281):
  - A row or card with `on:click` needs a keyboard way in: make its
    title a real `<button type="button">` (its click bubbles to the row's
    handler, so there is one navigation), or give a non-HTML target
    (an SVG `<g>`, a calendar cell) `tabindex="0"`, a `role`, an
    `aria-label` and an Enter handler.
  - No `opacity-0 group-hover:opacity-100` on a control. Always visible
    and muted.
  - A `<label>` has `for=` and its control the matching `id`; an input
    with only a placeholder gets an `aria-label`.
  - An error that appears after an action has `role="alert"`.
- **`draft::write_edit_draft` returns whether the draft was stored**
  (#281, 1.11). Word any "kept on this device" through `kept_note(kept)`
  in `node_editor.rs`; a full localStorage is a normal case.
- **A request that outlives its component must finish the user's work
  some other way** (#281, 1.6): the editor's image upload settles its
  placeholder in the *saved* text through `settle_closed_upload`, with
  `expected_updated_at`, when its signals are gone.
- **A failed load is `LoadError`, never the empty state** (#281, 1.7;
  `load_error.rs`). Don't write `.and_then(|r| r.ok()).unwrap_or_default()`
  or `.await.unwrap_or_default()` on the data a view's empty state reads:
  match `Some(Err(_))` first and render `<LoadError what=… on_retry=…/>`.
  Defaulting is fine only for a secondary read (a counter, a keyboard
  index) beside a view that shows the error.
- **An action's error is shown, never `let _ =`** (#281). A dropped error
  on revoke, delete or save tells the person it worked. Toast it. After
  the `.await`, use `try_` access.
- **Deleting a node goes through `DeleteConfirmModal` with `details`**
  (#281): `details=node_delete_details(show, ids)` loads the counts of
  what goes with the node and keeps Delete off until they are shown. A new
  place that deletes nodes must pass it; never call `api::delete_node`
  behind a bare confirm.
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
