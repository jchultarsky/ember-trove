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
  Project page and on the dashboard card (`compact`). The card shows the
  Brief and the open tasks. The dashboard no longer reads a Status
  section out of the body (retired 2026-11-12): don't bring the scrape
  back. On the Project page
  it leads, above the body (`SectionShell`'s `lead`; owner decision,
  2026-11-11): don't move it under the body. A new place that
  shows a Brief uses it, so the age and the stale marker are never left
  off. A write that changes two things (the Brief, then the body, for
  "Move Status section into Brief") saves the copy first and removes the
  original second: a failure in between leaves the words in both places,
  never in neither.
- **A project's state is the page's, not the chip's** (S6a).
  `NodeView` owns `project_state` and `project_state_at` and hands them
  to `ProjectStateChip` (`project_state.rs`), to the Focus control
  (`focusable` is derived from it) and to `TaskPanel`. A new place on the
  Project page that depends on the state reads that signal; it never
  keeps its own copy. `project_state::declare` is the one write, shared
  with the dashboard's Park / Make active buttons and Data health's
  fixes. The chip sets the state when a choice is made, before the server
  has it, so the page reads the server (the Focus entry) only from the
  chip's `on_settled`, never from an effect on the state (ERRORS.md,
  2026-11-13). The chip is one stable `<button>` with reactive
  attributes: a button rebuilt on each change would drop the keyboard.
  Its choices are a radio group like `SegmentedRadio` (one tab stop;
  arrows, Home and End move; Enter chooses), close on Escape and when
  focus leaves, and span the screen below `sm`. Active is the unmarked case: `ProjectStateBadge` renders
  nothing for it and the chip is its icon alone, so no row says "Active"
  twice (the label often does already).
- **The dashboard's Area groups hold active projects; parked ones go to
  `ParkedGroup`**, closed by default (owner decision, 2026-11-13). A
  parked card has `CardFocus::Absent` (it can't be in Focus) and a card
  in the Focus section has no state button (it may be someone else's
  project). Completing or cancelling is offered on the Project page
  only.
- **A My Day row decides with `MyDayTask::in_my_day` /
  `in_overdue_zone`**, never with the task alone: they carry the
  project's state. Both zones filter their own feed with the same rule
  (the feeds are fetched apart). `KanbanTaskRow` takes `project_state`
  and says "Project parked" (`quiet_project_badge_view`) on a row whose
  project is not active. `toggle_done` takes the project's state too:
  only a task that was carried into Today is stamped "done today".
- **Closing a project goes through `project_state::declare`** (S6b,
  #326), never `api::declare_project_state` from a view. It sends the
  plain request; a 412 means something under the project would close, so
  it fetches the plan and opens the app's one confirmation
  (`modals/cascade_confirm.rs`, mounted in `Layout`). `on_failed` also
  runs when the person goes back, so the caller puts back what it showed
  and says nothing. The catch-up on an already closed project is
  `project_state::ask_to_close` (the page's `StillOpenNotice`, Data
  health's "Close them…").
  - The dialog is opened with `cascade_confirm::ask`, through a
    thread-local handle like the toasts': the question is asked after an
    `.await`, where there is no context.
  - **No optimistic flip for a terminal state.** The chip shows Complete
    only when the server has it: while the dialog asks, the project is
    still what it was. Parking and re-opening still flip at once.
  - Its callbacks are `Fn + Send + Sync` and are kept in an `Arc`, not a
    `Callback`: they are stored after an `.await`, with no owner to
    dispose a `Callback`'s slot.
  - After a state settles the page re-reads what a cascade can have
    changed: the task list (`TaskRefresh`), History
    (`history_writes`, read by `HistoryPanel`) and the notice. A new panel
    that shows tasks or History on the project page joins `state_settled` in
    `node_view.rs`.
  - The dialog shows the server's plan and nothing else. Don't work out
    in the browser what would close. Whether it is a catch-up is the
    plan's `is_catch_up()` (the master already has the state), never the
    caller's belief: `ask_to_close` refuses to ask when the project is no
    longer in that state.
  - **A question belongs to the page it was asked on.** `declare` and
    `ask_to_close` read `current_path()` at the click, before any
    `.await`; `ask` refuses when the page has been left or another
    question is on screen, and an open dialog is withdrawn on navigation.
    Every path ends in exactly one of `on_saved` or `on_back`, or the
    caller's lock stays set.
  - **A button that opens the dialog is `aria-disabled` while busy, not
    `disabled`** (and ignores its click): the dialog returns focus to
    it, and a disabled button can't take focus. The same for the
    dialog's own commit button while it saves.
- **An open modal's keys must not depend on where the focus is.**
  `ModalShell`'s panel has `tabindex="-1"`, so a click on its text keeps
  the focus inside (Escape, the Tab trap). The cascade confirmation is
  also known to the window's key handler (`CascadeConfirm::is_open`):
  Escape goes back and no shortcut acts on the page behind it.
- **History is one section, `HistoryPanel`** (`history_panel.rs`, S6d,
  #329): the node's notes and the changes to it, merged by
  `common::history::merge`. It was the Notes section; its slug and test
  id are `history`.
  - **Two reads, never one empty state.** Notes come from `/notes`,
    changes from `/activity?scope=history`
    (`api::fetch_history_changes`, which also returns the server's
    "there are more" header). A failed notes read is `LoadError`; a
    failed changes read leaves the notes on screen with a line that says
    so. Don't fold the two failures together.
  - **The list is a keyed `<For>`, and its frame switches on a memoised
    phase** (loading / failed / loaded), not on the notes themselves. A
    read that brings notes again then redraws nothing but the cards
    whose key changed (a note's key carries its `updated_at`), so an
    open editor stays. Matching on the notes signal directly rebuilt
    every card on each answer, whatever the keys said. A saved note is
    put into the list in place (`NoteCtx::saved`); a failed re-read
    keeps the notes on screen and says so.
  - **Empty only when both reads say so**: "Nothing yet" needs the
    changes read to have answered well. While it is loading, or after it
    failed, an empty notes list is not an empty History.
  - **Anything on the page that writes History bumps `history_writes`**
    in `node_view.rs` (`history_changed`): the state chip
    (`state_settled`), the status chip (`on_saved`), the Brief
    (`on_changed`). A new control that changes a node's status, state or
    Brief does the same.
  - **A note's buttons come from its flags** (`ReaderNote.may_edit`,
    `may_remove`), never from who owns the node. The "edited" mark is
    `edited_at`; nothing reads a note's `updated_at` except the list key.
  - **A change line is text.** `activity_lines.rs` words every log entry
    for both History and Activity (`action_detail`, `change_details`).
    A status label and a Brief's words are free text: never
    `inner_html`.
  - A save of a note that changed nothing sends nothing.
- **A log entry this build can't read is skipped** (`api/activity.rs`,
  `known`): a tab open across a release must not lose a whole list to
  one new action.
- **Words**: History is what occurred about a node; **Versions** is its
  saved texts ("it stays in Versions"); **Activity** is the audit trail.
  Don't call body versions "history" on screen.
- **Focus is the viewer's own** (#282). `FocusToggle` / `FocusMenuItem`
  (`project_focus.rs`) take `state: RwSignal<Option<bool>>` and render
  nothing while it is `None`: a control that guessed "not in Focus" would
  show the wrong word. The dashboard's Focus section and its cards are in
  `project_dashboard.rs` (`FocusSection`, `CardFocus`); a Focus card is a
  `ProjectCard` with its strip, never separate markup. A card in the
  Focus section may be a project shared with the viewer, so it has no
  owner-only control (no pin star).
  The Focus cards are a keyed `<For>`: a move reorders them in place.
  Don't rebuild them from the fetched list on a move (a why-line saved a
  moment ago showed as it was before). Reorders are sent one at a time,
  the latest last. The why-line saves through `set_focus_why`, which
  never adds an entry; a control whose state failed to load offers a
  retry (`FocusRetry`). The age and its tooltip use the reader's own
  calendar (`local_date`, `local_today`), never UTC.
- **Task dependencies live in the shared `TaskEditor`** (S5d, #338):
  `TaskDependenciesSection` (`task_dependencies.rs`) is its third child,
  so My Day and the node page both have it. What it changes **applies at
  once** and is not part of the draft (the ☀ button's precedent); it is
  set apart and says so. Don't route it through `TaskEditDraft` or Save.
  - The row owns its blocked count as a signal (`blocked_by`), and the
    section sets it from what it loads and after each change. Don't bump
    the list's refresh from the section while it is open: a refetch
    rebuilds the open editor. It bumps `TaskRefresh` once, on unmount,
    if something changed: a row rebuilt later from the list's own data
    (a reorder, a zone folded and opened) would otherwise show the old
    count.
  - After a change the section puts the focus back on itself when it is
    left on nothing (the pressed remove button went with its row).
    Otherwise the next key goes to the page's shortcuts: Escape left the
    node page, and My Day's `d` deleted a task. My Day's key handler
    also stands down while any task editor is open.
  - A linked task's title goes to the task's row when it is on this page
    (`focus_task::focus_task_here`), and navigates only otherwise: a
    navigation to `?task=` on the page already shown does nothing (the
    router doesn't run a route again for a new query). A modified click
    is left to the browser.
  - The section's parts each read the lists; the picker reads only
    `may_edit`. A change replaces the lists and must leave the picker
    mounted, with its focus.
  - Controls follow the API's `may_edit`, and the "not shown" line its
    `not_shown`: nothing about who sees what is decided in the browser.
  - `TaskPicker` is its own combobox: it answers an empty query, and asks
    nothing before it is focused. A click must open its list as well as
    a focus (after an add it still has the focus), without asking again
    (`arm_and_open`: setting a signal to the value it has still notifies).
    Ctrl/⌘+Enter is left to the editor (Save); only the main mouse
    button picks.
  - The "Blocked" chip is `blocked_badge_view`, shown where
    `task_dependency::is_blocked` says. Neutral tone: it is information,
    and the task's place in every list is unchanged.
  - A linked task's title is plain text (`markdown_inline_plain_text`).
- **A node's links are read once for its page** (S7a, #342):
  `NodeLinks` (`node_connections.rs`), made in `NodeView` on the
  home-and-links counter, and handed to `ConnectionsPanel` and
  `UnlinkedMentionsPanel`. A section calls `links.wanted_while(open)`;
  nothing is read until one opens. A new section that needs the links
  takes the same `NodeLinks`; it never calls `fetch_edges_for_node`
  itself. The groups, their order and their headings are
  `common::connections` (`grouped`, `Group::heading`); don't sort or
  name them in the UI.
  - A group's heading is an `h3`: the `h2`s are the page's sections.
  - The Home group is `HomeRow`; the list leaves it out wherever that
    row shows it with its controls (`listed`). It stays for a Reference
    (no row) and for a reader who may remove the home's link but is not
    the row's `can_edit`.
  - A row removed or linked from the page leaves its list at once
    (`just_linked`), not when the next read lands: until then its button
    is live, and a second click repeats the action.
  - An empty list says "none" only when everything was looked at
    (`stopped_short`).
  - A remove control is rendered only when the row's `may_remove` is
    true (the server's answer). Never offer one by `can_edit` alone.
  - "Add Edge" is offered when the reader may write on the node
    (`may_add`): `can_edit` (owner or admin), or their own grant is an
    editor's (`common::permission::holds_editor` over
    `GET /nodes/{id}/permissions`, which lists a non-owner only their
    own rows). Unknown or unreadable means not offered. A new write
    control on a node page that a shared editor may use goes the same
    way; `PermissionRole::satisfies` is the API's rule too.
  - A row is an `<a href>`, not a `<button>` with a navigate call.
- **Don't read a memo and a memo derived from it in one render closure
  or effect** (S7b, 2026-11-17). With `unmet = Memo(…)`, `shown =
  Memo(unmet > 0)` and a closure that reads `shown`, then `unmet`:
  checking `shown` recomputes `unmet` without marking the closure (a
  memo that changes marks its subscribers dirty, all but the one
  reading it at that moment), `shown` doesn't change, and the closure
  never runs again. The node page's "Blocked" chip kept saying "2"
  after a dependency was removed. Make the derived one a
  `Signal::derive`. A resource has nothing to do with it: a `Memo` over
  a `LocalResource` does run again when the read lands (the first
  version of this rule said otherwise, and was wrong).
- **Blockers on the page** (S7b, #346; rules in `common::blockers`):
  - The header chip (`NodeBlockedChip`, `node_connections.rs`) counts
    with `unmet_dependencies`: `ViewedNode::blocked_by` until the links
    are read, `blockers::direct` of them after. Never count a
    dependency in the browser any other way: the server holds its count
    equal to that one.
  - The chain (`GET /nodes/{id}/blockers`) is read only while
    Connections is open, the node `still_waits` and a dependency is not
    met. It renders in its own `<Suspense>` inside the list, so the
    list doesn't blink.
  - A "Depends on" row's word is `Connection::standing_in(group)`; its
    order is `common::connections`' own. A chain row has no remove
    control: it is not this node's link.
  - The chip is `task_row_scaffold::blocked_chip` / `BLOCKED_CHIP`: one
    look for a task, a node and a card. Neutral, never a warm colour.
- **Links not shown** (#349): the page keeps one
  `RwSignal<u32>` (`links_not_shown` in `NodeView`), set from
  `ViewedNode::links_not_shown` and then from what the remove answers.
  The header's `LinksNotShownChip` and the line in `ConnectionsPanel`
  (`not_shown_line`) both read it. The server sends 0 to anyone who may
  not edit the node, so neither asks about rights. The line sits
  outside the list's `<Suspense>`: it must show beside "No connections
  yet". The confirmation is `data_health_view::InlineConfirm`.
- **A refused person save** (S8a): `EmailTakenNotice` names who has the
  address and never offers to save anyway. It navigates with the dialog
  **still open**, so the unmount keeps what was typed as a draft (#170);
  closing first drops it, as every closing does. The rows the refusal is
  about are marked through `ContactListEditor`'s `flagged` (`Flagged`):
  a mark is worked out from what the rows hold now (`Flagged::marks`),
  and a row's input calls `flagged.notify()`, so it goes when the row is
  put right. The same address twice is caught by `listed_twice` before
  anything is sent. Both key a typed value with `typed_key`, which trims
  first, as the API does. After a refusal the keyboard goes to the first
  marked address when it fits under the notice, and to the notice when
  it doesn't (`focus_flagged`): neither may be scrolled out of view. A
  new alert in that form needs `tabindex="-1"`.
- **What the notice offers is a `TakenOffer`** (S8b, #357), chosen by
  the dialog's `PersonForm`: `CarryOver` for a new person, `AddHere` on
  a node (`OnNode`, which offers opening only, when the reader may not
  add people there), `Merge` for an edit of a saved person (S8c).
  - **Carry-over** is `common::person_carry::carry_into`, pure. The
    typed input travels in memory (`draft::PersonHandOff`, taken once
    when the other person's dialog mounts) and is merged in the restore
    effect, against the person as fetched or an unsaved edit of them
    the device kept (`Carried.on_kept_edit`, which the review says).
    Never merge at the click, and never write the typed input to a URL
    or a new storage key.
  - The report rides the dialog's own draft (`PersonFormDraft.carried`).
    A reload restores it. The same source again (`source_fp`) is shown
    as it was left, not merged again; another source joins the report
    (`CarryReport::joined`).
  - The new-person draft is dropped only when the other person is saved,
    and only if it still holds what was carried
    (`clear_person_draft_if`). `carry_to` refuses when the draft can't
    be stored (`try_keep`): backing out must have it to go back to.
  - "Added" marks are `carried`, not `flagged`: neutral, never
    `aria-invalid`, and about the value (`CarryReport::marks`, `fills`).
  - **`AddHere`**: the panel that owns the dialog does the add
    (`NodePeoplePanel::add_existing`) and closes the dialog only once it
    has succeeded. Offered only when the reader may add people there
    (`can_add`); a person already there is said, not offered.
  - One set of signals serves every opening of that dialog, so an
    answer acts on it only if it is still the opening the button was
    pressed in (`AddHere.opening`, bumped by the editor). Anything else
    that awaits and then touches a dialog checks the same way.
  - A button that starts a request is `aria-disabled`, not `disabled`,
    while it runs (a disabled button drops the keyboard), with a guard
    in the handler. When an answer removes the focused control, the
    keyboard goes to the form's alert (`focus_alert_if_lost`).
- **Merging two people is one dialog** (S8c, #363):
  `modals/merge_people.rs`, mounted in `Layout`, opened with
  `merge_people::ask(MergeAsk { kept, other, among, .. })`, like the
  cascade's. A new place that merges people asks it; never call
  `api::merge_people` from anywhere else.
  - The words are `common::person_merge::plan_lines` (the kept person,
    the other: two records of one person often have one name, so a
    sentence never relies on the names) and `MergeRefusal::says`. With a
    refusal there is no Merge button.
  - The server's plan is the only thing shown, and its token the only
    thing confirmed. A 412 reads the plan again and says it changed.
  - **Never two dialogs at once.** From the person editor's notice the
    merge dialog takes the editor's place (`merge_with`): going back
    reopens the editor with what was typed; a merge does not save it,
    and the notice says so. The draft stays stored while the merge
    dialog stands in (`standing_by`), so a reload there keeps it. Reopening the editor
    after a merge with what was typed would write the old lists over
    what the merge added.
  - After a merge the dialog drops both people's edit drafts. The
    person page then shows the one kept, with `replace` when that is
    another page: Back must not land on a person who is gone.
  - Its Escape is the window dispatcher's too (`layout.rs`), as for the
    cascade: it deletes something.
- **A person's save carries the version it was made from** (S8c):
  `PersonEditor`'s `based_on`, the `updated_at` the page read, sent by
  `api::update_person`. A 412 or a 404 on an edit is
  `stale_save_message`: it says to reload, and promises a kept draft
  only when `try_keep()` stored one. Any other write of a person from
  the browser sends its version the same way.
  - **The version is read when Save is pressed, so it proves nothing
    about the form.** A form that was set aside and comes back (a kept
    draft, Undo, back from the merge dialog) may have been read before
    the person changed, and would pass the check and write the old
    values back. Every such path goes through
    `PersonFormDraft::rebased_on` (`put_back`, and the restore effect):
    only what was typed, on the person as they are now, with the
    `CHANGED_SINCE` notice (`rebased` keeps it across a reload). A new
    way to bring a form back does the same.
  - After a 412 the merge dialog ignores Merge for `SETTLE_MS`: a
    second press meant for the old plan must not confirm the new one.
- **A mention's link is `MentionName::link_target()`**, never
  `person_id`: after a merge the token's id can be an alias, which is
  no page.
- **Mentions of people** (S9, #360). `common::mention` reads
  `@[label](person:id)` for the API; never scan for `@[` anywhere else.
  - **A mention is shown only where both readers agree it is one**
    (`ui/src/mention.rs`, `with_mentions`): `common::mention::mentions`
    reads a token at that place in the text **as written**, **and** the
    Markdown parser read the same bytes as an inline link, outside an
    image. That is true of one reader's page. Don't build on "every
    reader's Markdown reads the same links": it does not (see "Nothing
    writes a person's current name" below).
  - **The text is scanned for mentions once, as written** (`Token::all`),
    and each one is found again by position after the wiki links are
    written as HTML (`wikilink::Moved`). Never scan the rewritten text:
    it holds other tokens (a wiki link's HTML changes which backticks
    pair), and the API, which decides who is mentioned, reads the text
    as written (fifth review of #362).
    It is replaced among the parsed events: a fixed opening tag, the
    name as a `Text` event (the HTML writer escapes it), a closing tag.
  - **The sanitizer is the last thing that touches rendered HTML.**
    Two designs were tried and must not come back (both reviews of
    #362):
    - HTML for the name put into the Markdown **source**: a token
      inside a writer's own `<a href='https://elsewhere/?q=…'>` carried
      the person's name to that address on a click.
    - HTML put in **after** the sanitizer, at markers in the text: a
      writer forged the markers, and the fill cut through sanitized
      markup, leaving attributes of the writer's choosing on an
      allowed element.
    Anything new that a renderer adds goes in as events, before the
    HTML is written.
  - **The result is checked, not trusted** (`Resolved::intact`; third
    review of #362). Markdown and the HTML parser read a text
    differently: raw HTML can end inside an open attribute
    (`<div><a href='https://elsewhere/?q=`, a blank line), and what
    Markdown writes next is then that attribute's value to the
    sanitizer. No rule about where a token sits lists every such case.
    So every mention written must be in the sanitizer's output as the
    element written for it, as often as written. One that names nobody
    is checked too: its label is a name someone picked, and a page is
    not the same for every reader (a wiki link is an `<a>` for one and
    a `<span>` for another, and inside `<svg>` the HTML parser treats
    them differently). The check reads the output and never changes
    it. Anything else that puts a reader-only secret into rendered
    Markdown needs the same check.
  - **A text that fails the check is shown with its raw HTML as text**
    (`markdown::checked`, `RawHtml::AsText`): nothing of the writer's
    is then HTML, so there is no attribute to fall into; the mentions
    are mentions and the writer's link is no link. Both attempts parse
    **the same text** (wiki links written as HTML; their own tags
    stay, `Moved::wrote`), so Markdown reads the same links in each.
    `checked` never returns an attempt that failed the check; were all
    to fail (none has), it renders nothing.
  - **The check is one pass over the page** (`Resolved::intact`): the
    writer chooses how many different labels a text has, and a search
    of the page for each took a third of a minute on a body of the
    size the API allows. Anything checked per mention must not search
    the whole page per mention.
  - **The element it looks for can't be typed** (`Resolved::mark`;
    fourth review). Each render makes a random class, writes it on
    every mention, and allows it in that render's sanitizer alone
    (`sanitizer_naming`). Without it a writer typed the
    element once per guessed name beside a mention made to fall into
    their link's address, and the right guess made the count come out.
    So rendered markup is `class="mention m-<32 hex>"`: match
    `.mention` to find a mention, never `[class="mention"]` (that is
    exactly a copy a writer typed). Only the look of a link to a person,
    and the test of it, match on the start of the attribute
    (`[class^="mention m-"]`). Never allow a class in the sanitizer
    that a text could know and a check relies on.
  - Test a claim like "never in an attribute" by walking the output's
    attributes (`attribute_values` in the tests) and with the
    fixed-sequence fuzz, not with `contains("href=\"https://evil")`.
  - The sanitizer allows the mention's classes by name (`mention` on
    `a` and `span`, `mention-unresolved` on `span`), so a writer can
    type them and give their own words the plain look of a name, as
    they can make them bold. That is accepted: no name is told by it.
    The look of a link to a person is styled only for an element that
    carries a render's own class and leads to `/people/…`
    (`a[class^="mention m-"][href^="/people/"]` in `input.css`): the
    sanitizer strips every `m-…` but its own render's, so a writer's
    `<a class="mention" href="/people/…">` does not get it.
  - A token inside a link the writer wrote in HTML names nobody
    (`anchors_opened`): it is written with its label, and checked like
    the rest. That is tidiness, not a boundary: it reads tags by eye
    and can be fooled. Don't leave such a token to Markdown as a plain
    link: unchecked, its label fell into the writer's address.
  - `mine` → `<a class="mention" href="/people/{id}">`; otherwise
    `<span class="mention">` with no id in the markup; nobody to name →
    `<span class="mention-unresolved">` with the label. Told apart
    without colour (`input.css`).
  - `render_markdown` and `render_body` take the people to name;
    `render_note` is for notes (no wiki links there). Anything else
    (`render_markdown_plain`: a Brief, a template) shows "@name" with
    no link, because the sanitizer drops the `person:` scheme.
  - **The names come from the API with the text** (`ViewedNode.mentions`,
    `ReaderNote.mentions`, `SavedNote`). Don't add a "resolve these ids"
    call: there is deliberately no such endpoint.
  - **The editor names only who the text came with and who was picked
    in it** (`MentionPicker::set_known`, `names`), never "everyone in
    my registry". A token someone else typed with the id of the
    author's person is not a mention; the preview must not show it as
    one.
  - **Nothing writes a person's current name into a text.** A label is
    the name as it was when the mention was picked; the page shows the
    current one. An "Update" for labels that show an older name was
    built and taken out (2026-11-19). It wrote the name into every
    token the editor's page showed as a mention, and three reviews in
    a row found a text for which one of those was, to another reader
    or after the edit itself, the address of a link: the Markdown
    parser reads a text differently by its length and by which wiki
    links lead somewhere (a link label's limit of 1,000 counted
    characters, a budget for reference links that the text's size
    sets). Before building it again, give the parser one text for
    every reader (a placeholder of fixed shape for each wiki link,
    swapped after the parse), and check the text again after the
    edit. It belongs with the merge tool (S8c).
  - **The picker is `MentionPicker`** (`mention_picker.rs`), one hook
    for the body textarea and `ResizableEditor` (`mentions=` is opt-in:
    notes only). The textarea calls `on_keydown` first and returns if
    it was used.
    - People are offered by `common::mention::picker_match`: a word of
      the name, or the start of an email address. Never by company or
      phone (`"lunch @12"` offered whoever had 12 in a number).
    - Enter and Tab pick only a highlighted option; one is highlighted
      only when a **name** matches what was **typed**. A bare `@`, an
      address-only match, or a list that opened because the cursor
      moved into an `@…` already in the text (`after_click`,
      `on_keyup`; `by_typing` false) needs an arrow key first: Enter at
      the end of a line that ends in `@bob` is a new line. A paste, a
      drop and an undo are not typing either: `after_input` takes the
      event and reads its `inputType` (`common::mention::typed_by`).
      Nor is an edit the picker itself makes (`editing`). Before a
      pick, Enter and Tab check the list is still about where the
      cursor is.
    - Escape closes the list only (it must not reach the editor's
      Escape), and the "couldn't load your People" alert the same way.
      Ctrl/⌘+Enter stays the editor's and closes the list; any other
      key with a modifier is left alone (AltGr types letters, and `@`
      itself, on many keyboards). Shift+Tab leaves the field.
    - A pick is one `insertText` edit by the browser (`edit`), so
      Ctrl+Z undoes it and the box's own `input` handler sets the
      signal. Don't `set_value` a textarea for an edit the user may
      want to undo, and don't replace the whole text for a small
      change: the box's `input` handler then sees the cursor at the
      very end.
    - Offsets from the box (`selectionStart`) count in the box's own
      text (`el.value()`), not in the editor's signal: a text saved
      with CRLF line ends differs until the first keystroke.
    - The registry is read when an `@` starts a mention: at the first
      one, and again at each later one (a person added since, a read
      that failed). Never per keystroke: `reads` is bumped only when
      the `@` is a new one (a signal set to the value it has still
      notifies).
    - The highlight is derived from the list as it is
      (`active_in`): whoever an arrow key moved to, **by person**, else
      the first when a name matches. So it is right when the registry
      arrives after the name was typed, and Enter never picks someone
      who slid into a highlighted row.
    - `on_keyup` follows the cursor on an arrow, Home or End.
    - Option ids are keyed by person, and an `aria-live` line says how
      many people there are and who is highlighted.
    - The list docks under a note's box (`Dock::Below`): above it, the
      Notes page cut it off.
  - **A stale label is never rewritten**, in a body or a note. The
    editor's baseline, Revert, the 412 fingerprints and draft recovery
    all hold "the text as loaded".
- **A bulk action is paced by request** (`task_actions::paced`; S7a):
  a job that is more than one request uses `paced_by` with its cost
  (`FilingJob::requests`). A bulk button checks the card's `busy` before
  it starts, as a row's does.
- **A fix that is two writes saves the copy first and removes the
  original second**, and on a failure reads the report again: the first
  write may have gone through (`Fix::File`, `apply_filing`).
- **The archive notice** (`ArchivedNotice`, `status_picker.rs`) is read
  when Archived is chosen. A failed read says it couldn't check, never
  nothing: silence there reads as "nothing uses it".
- **A node's home is shown and edited in two places that share one
  counter** (2026-11-12): `HomeChip` in the page header and `HomeRow` in
  Connections (`node_home.rs`). `NodeView` owns the counter
  (`home_refresh`) and passes it to both, so a change in one shows in the
  other. A third place that changes a home bumps the same counter.
- **Task states in the UI (S5a, #314).** The state is set in the one
  `TaskEditor` through `StatePicker` (`TaskStatus::PICKABLE`: Open,
  Waiting, Cancelled); Done is only ever the checkbox
  (`task_actions::toggle_done`). The editor sends the state only when it
  was changed there (`TaskEditDraft::take_request`), and shows no picker
  for a done task. Rows say Waiting and Cancelled with
  `state_badge_view`; the Project page's task list also shows a done
  task's day (`completed_badge_view`).
  The draft's state is unlike its other fields: it changes outside the
  editor (the checkbox, another tab). `TaskEditDraft` therefore keeps the
  task's own state (`base_status`), which `EditSession` refreshes with
  `follow` each time it hands the draft out, and sends a state only when
  the picker differs from it (`common::task::changed_state`). Never
  compare against "what Save last sent": a Save that failed would drop
  the state on the retry.
  A new small picker is a `SegmentedRadio` with its own `Segment`s, like
  `PriorityPicker` and `StatePicker`: don't copy the radio-group markup.
  Waiting is cool (`WAITING_TONE`), never warm. My Day's backlog is
  "everything still to do that is not `in_my_day`", not "no focus date".
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
     more…", Undo).

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
  renderer (`wikilink.rs::preprocess_wikilinks_moved`) and the excerpts use
  `replace_wikilinks`; don't scan for `[[` in the UI. The editor's `[[`
  autocomplete is the exception: it looks backwards from the cursor.
- **Rendered Markdown keeps only classes the renderers write, by name**
  (2026-11-10): `markdown::sanitizer` allows `wikilink` and `mention`
  on `<a>`, `wikilink-unresolved`, `mention` and `mention-unresolved`
  on `<span>` (and each render's own random class, `sanitizer_naming`)
  through `add_allowed_classes`, and nothing else: the app's own
  stylesheet has classes that cover the screen. A writer can type the
  named ones, so none of them may place or size anything. A renderer
  that needs a new class adds it there, by name.
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
  what goes with the node and keeps Delete off until they are shown.
  `DeleteDetails::Counted.warning` (S7a) is the preview's "still used
  as a source" line: shown apart from the losses, and never a block. A new
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
