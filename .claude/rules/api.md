# API / Backend Rules (auto-relevant for `api/`)

Axum 0.8 + Tokio + sqlx 0.8 (PostgreSQL 16) + utoipa (OpenAPI). Post-edit gate:

```
cargo check && cargo clippy -- -D warnings
```

## PostgreSQL / Axum patterns

- **`Query<T>` with `Vec<Uuid>`:** Axum/serde won't deserialize repeated query params
  into a `Vec` cleanly. Take `Option<String>` and `.split(',')`-parse server-side.
- **`node_type` serde:** lowercase variants — `"article"`, `"project"`, `"area"`, etc.
- **`Option<Option<T>>` PATCH semantics:** distinguish *absent* (leave unchanged),
  *null* (clear), and *value* (set) with the `deser_double_opt` deserializer.
  Real code: `common/src/task.rs` (`deser_double_opt`, used on `UpdateTaskRequest`).
  See `.claude/patterns/double-opt-patch.rs`.
- **Static AND/OR tag SQL:** bypass empty filters with
  `array_length($n::uuid[], 1) IS NULL` and use a `HAVING` clause for the AND case.
- **All SQL is parameterized** (sqlx bind params) — never string-format user input into queries.
- **Migrations** live in `migrations/` (sequential `NNN_name.sql`), auto-applied at API
  startup via `sqlx::migrate!()`. Add a new numbered file; never edit an applied migration.
- CI validates migrations against `postgres:16` with `sqlx-cli` **pinned to 0.8.6** to
  match the `sqlx` dependency.
- **A migration's test stands on the schema before it** (2026-11-15):
  `#[sqlx::test(migrations = false)]`, then
  `apply_migrations(&pool, |v| v < N)`, seed, run N, compare, then
  `|v| v > N` before any repo call. Rebuilding the old shape by hand on a
  fully migrated database hides everything the migration does that a
  second run leaves alone (a changed default, a dropped index): the
  053 test missed both until it was rewritten this way.
- **Data backfills that need Rust logic** (#143): put them in `api/src/backfill.rs`,
  not SQL regex. Each one claims a `data_backfills` row in its own transaction
  (`INSERT … ON CONFLICT DO NOTHING`; claimed = `rows_affected() == 1`), so it runs
  once even with two instances starting. It reuses the tested `common` parser, and
  failures roll back and retry on the next start.
  - A rewrite of user content snapshots the old body into `node_versions` first.
  - It sets `SET LOCAL ember.keep_updated_at = 'on'`, so the
    `touch_updated_at` trigger leaves `updated_at` alone.
  - Test it in `pg_tests` with nodes inserted by raw SQL.
  - `status_into_brief` (2026-11-11) is the second one: a project's
    Status section becomes its Brief. It dates the Brief by the node's
    `updated_at`, not by now: a backfill must not make old words look
    fresh. Give a backfill's snapshots their own `created_by`
    (`system:backfill-…`).

## Optimistic concurrency (#161 follow-up)

- `PUT /nodes/:id` takes an optional `expected_updated_at`. When it is set, the
  `UPDATE` adds `AND ($8::timestamptz IS NULL OR updated_at = $8)`. No row
  means a missing node or a stale version: the repo checks `EXISTS` and
  returns `EmberTroveError::PreconditionFailed`, which the API sends as
  **412**. 409 stays reserved for duplicate titles.
- The version check runs **after** `require_editor`, so a viewer or a
  stranger can't use 412 vs 403/404 to probe a node. The stub repo in
  `tests.rs` 412s on any precondition; it proves authZ comes first.
- `updated_at` is the version (the `touch_updated_at` trigger). Only node
  writes bump it (`update`, `set_pinned`). Anything new that writes `nodes`
  moves the version too: the editor then sees its own text at a new version
  and re-sends, at one extra round trip.

## Edge write rules (#224)

- `POST /edges` checks access first: editor on the source, and on the
  target **editor for `contains`** (it claims the child's one home) and
  viewer otherwise. Only then come the kind rules
  (`common::ontology::edge_kind_violation`) and the repo's structural rules
  (`structural_refusal`: a second home, a loop). All refusals are 422 with
  `EdgeRefusal::message()`, which names no node.
- The structural checks and the insert share one transaction holding
  `pg_advisory_xact_lock(hashtext('ember.edges.' || type))` for acyclic
  types (`EdgeType::is_acyclic`). Without it, two concurrent inserts close
  a loop: `concurrent_inserts_cannot_close_a_loop_together` fails every
  run with the lock removed.
- The loop check is a recursive CTE with `UNION`, so a legacy loop
  terminates. The home check skips the same parent, so a repeat stays a
  409 duplicate.
- `DELETE /edges/:id` refuses a `wiki_link` with 422 (#281, §6.6): it
  mirrors `[[Title]]` in the text, and sync would put it back. The check
  comes **after** the rights check, so a stranger still gets 403 and
  learns nothing about the type. `EdgeType::is_machine_owned` is the one
  rule; create, convert, `LinkRef::removable` and the UI's remove
  controls all read it.
- `DELETE /edges/:id` for `contains` and `related_to` accepts an editor of
  **either** end (one home must never trap a child; `related_to` has no
  direction). Other types stay source-editor only. The rule is
  `EdgeType::deletable_from_target`, shared with convert and Data health.
- `POST /edges/:id/convert` (#228) is decided by `common::edge::convert_ends`:
  rights to delete the old link come first, then the conversion is checked
  (only to `references` / `related_to`, never from a wiki link), then the
  rights to create the new one. The route reads each end's rights with
  `end_rights`: only a 403 means "no", and any other error propagates. The
  repo deletes by id **and** type, so a link changed since the checks is a
  404, and inserts with a bare `ON CONFLICT DO NOTHING`, so an existing
  link absorbs it.
- Data health edge items carry `links: Vec<LinkRef>`, judged by ownership
  like #216's memberships. `LinkRef::removable` and `convertible_to` reuse
  the API's rules, so a button is only offered when the call would pass.
- `related_to` is stored once per unordered pair (#226): the partial
  unique index `edges_related_to_once` on `LEAST/GREATEST(source, target)`
  answers a reverse insert with the same 409 as a duplicate. Restore folds
  pairs with `common::edge::fold_related_pairs`, migration 041's rule; the
  041 pg test asserts the two agree. Tests that need a legacy both-ways
  pair call `allow_legacy_related_pairs` (drops the index).
- One home per node (#228): the partial unique index `edges_one_home`
  (migration 042) on `target_id WHERE edge_type = 'contains'`. A second
  home is still refused first by the locked check (422 `SecondHome`), and
  `create` maps the index to the same 422. Restore folds homes with
  `common::edge::fold_homes` (oldest kept, others into
  `edges_removed_042`), migration 042's rule. Tests that need two homes
  call `allow_legacy_homes`.
- `GET /nodes/:id/home` names the parent only to someone who can view it.
  A hidden home goes to the child's **editors** as `node: null` (owner
  decision, #228), and to viewers not at all. `PUT` checks editor on the
  child, then editor on the new parent, then the kinds; the repo checks for
  a loop under the `contains` lock and deletes every other home, hidden
  ones included (§6.1).
- The restore preview counts rule breaks with
  `common::data_health::graph_rule_counts` and warns; it never refuses
  (owner decision).

## Captures (#234)

- A capture is unclassified Inbox input (`captures`, migration 043). Notes
  always have a node now (`node_notes.node_id NOT NULL`).
- **Owner-only, no admin bypass**, like People: `CaptureRepo` takes the
  owner as `&str` and every query matches `owner_id`. Another account,
  admins included, gets 404. Only `list_all` (the admin backup) crosses
  owners.
- `/inbox/quick` keeps its path and wire format for installed service
  workers; `common::inbox::capture_text` builds the text.
- Restore accepts formats 3 to the current one (`OLDEST_RESTORABLE`). A format-3 note
  without a node becomes a capture (`capture::from_nodeless_note`, 043's
  rule), and so does a task without one, from an archive of any format
  (`capture::from_nodeless_task`, 054's rule; S5c). Restore also clears
  the live captures and people, which the `nodes` cascade never reached.
- `POST /captures/{id}/convert` (S4b): the route checks the destination
  first, as its create route would (editor on the node or home, the
  containment table, `Capability::Tasks`). Then `CaptureRepo::convert`
  runs one transaction: it **deletes the capture first** (no row: 404,
  nothing made), then inserts through the shared executor-generic helpers
  (`insert_node`, `insert_grant`, `insert_home`, `insert_note`,
  `insert_task`). A failed insert rolls the delete back. Reuse those
  helpers for any other write that must share a transaction; don't copy
  the SQL.

## Account lifecycle (#239)

- Cognito sits behind `admin::UserDirectory`; route tests use the
  recording `FakeDirectory` in `tests.rs` (one step log shared with the
  stub `AccountRepo`, so tests assert order across both).
- **Suspension fails closed.** Suspend writes `suspended_accounts`, then
  the in-memory `SuspendedSet`, and only then asks Cognito (disable +
  global sign-out). Reactivate asks Cognito first and drops the row last.
  A Cognito failure after the row is a 502 `ApiError::Upstream` that says
  the account is suspended here.
- `reject_suspended` is the layer just inside `require_auth`: a suspended
  `sub` gets the same 401 as a bad token. The set is loaded at startup (a
  failed load stops startup) and re-read every 60 s through
  `SuspendedSet::refresh`, whose generation check drops a read that raced
  a route's change. Always write the row before touching the set.
- An admin can't suspend, delete or demote their own account (422).
- `{id}` must be the canonical lower-case UUID (`account_id`): Cognito
  also accepts an email alias or another casing, which would miss the
  self-guard and the set.
- Delete suspends first and ends with `mark_deleted`; the row stays a day
  (the longest ID-token life), then the daily sweep drops it.
- **Purge** (`AccountRepo::purge`, one transaction, advisory-locked per
  subject): delete order is suspend → email → purge → drain → Cognito
  delete → `mark_deleted`. Settings are deleted *before* the nodes so the
  counts match `deletion_preview`. Anything new keyed by an account must
  join the purge (`SETTINGS` or its own step); the pg test
  `purging_an_account_removes_only_its_data` sweeps every
  `owner_id/subject_id/created_by/granted_by/suspended_by` column and
  fails on a leftover. Traces on others' nodes become `DELETED_ACCOUNT`.
- **Stored files go through the `object_deletions` outbox.** The
  `attachments_queue_object` trigger (migration 044) queues a file
  whenever its attachment row is deleted, by any path (cascade, purge,
  single delete, restore), so code never queues by hand. The one
  exception is an upload whose row insert fails: it calls `enqueue`.
  `state.drain_object_deletions()` runs one background drain at a time,
  batching until the queue is empty; the daily loop retries. `due()`
  first drops keys that a row names again, so a restored file is never
  deleted. Never log an `s3_key`: it contains the file name.
- The purge takes `FOR UPDATE` on the account's nodes after its advisory
  lock, and the daily sweep re-purges deleted accounts before dropping
  their rows (to catch in-flight writes).
- `/auth/callback` and `/auth/refresh` sit outside the layer, so they
  check `suspended_session` themselves (callback → Cognito logout URL,
  refresh → revoke + 401). Any new public route that mints a session must
  do the same.

## What a backup holds (#281, format 5; Briefs made it 6, Focus 8, task states 9, project states 10, a note's edited date 11, no repeat rule 12, task dependencies 13)

- **One snapshot.** `create_backup` reads everything through
  `backup::read_snapshot` on `begin_snapshot`'s transaction
  (`REPEATABLE READ, READ ONLY`). A table read outside it can disagree with
  the rest, and that archive then fails to restore on a foreign key. So a
  new backed-up table gets a free `list_all_on(executor)` function in its
  repo module (the trait method delegates to it) and one line in
  `read_snapshot`. Don't read it through `state.<repo>`.
- **A new table that holds account data must join the backup**, as it must
  join the purge. In `BackupData` it is an `Option<Vec<_>>` with
  `#[serde(default)]`: `None` means an older archive that never carried
  it, and `restore_into` then leaves the live rows alone; `Some` replaces
  them (`restore_account_settings`). A table no node cascades to also
  needs its own `DELETE` in `restore_into` (people were missed until #281).
- **Attachment files are stored under `attachments/<attachment id>`**
  (`common::backup::attachment_entry_path`). Formats 3 and 4 used
  `attachments/<node id>/<file name>`, which two attachments can share.
  Restore maps entries to rows only through
  `common::backup::match_attachment_entries`; never match by file name.
- **No credential goes into an archive** (owner decision, 2026-11-09): an
  admin can download one. `repo::webhook::list_all_for_backup` never
  selects `secret`, and `BackupWebhook` has no field for it.
  - Nor a webhook's **address** (format 7, owner decision 2026-11-10):
    many are credentials in themselves. `BackupWebhook.url` is
    `skip_serializing`; it is only read from a format-5 or -6 archive.
  - On restore, a webhook whose id **and owner** match a live one keeps
    the live address and secret
    (`restored_active_over_live`). Any other comes back switched off
    with `WEBHOOK_URL_NOT_IN_BACKUP` (`restored_active`).
  - The restore's webhook errors never carry the driver's message: a
    failed statement can quote an address.
- **Project Briefs (#282, format 6)** are `briefs: Vec<ProjectBrief>`,
  read by `repo::brief::list_all_on` inside the snapshot. A plain `Vec`,
  not an `Option`: a Brief hangs on its node, the nodes are replaced by
  any restore, so an older archive can't leave live Briefs behind.
- **Focus (#282, format 8)** is `focus: Option<Vec<BackupFocus>>`, read by
  `repo::project_focus::list_all_on` inside the snapshot. It is keyed by
  an account *and* hangs on a node, so the cascade empties it on any
  restore. `restore_into` therefore reads the live entries **before**
  `DELETE FROM nodes` when the archive has none (`None`), and puts back
  those whose project the archive restores. `Some` replaces them. An
  entry whose node isn't restored is skipped.
- **Task states (S5a, format 9)**: a task carries `completed_at` and may
  be `waiting`. Older archives still restore: `TaskStatus::Open` reads
  `"in_progress"` (a serde alias), and a missing `completed_at` stays
  empty. Restore writes `Task::restorable_completed_at`, so a date on a
  task that isn't done can't fail the CHECK.
- **Project states (S6a, format 10)**: a node carries `project_state`
  and `project_state_at`. Restore writes
  `common::project_state::restored`: the archive's state for a Project,
  the one its **restored** label says when the archive has none (formats
  3 to 9), and never a state on another kind (a hand-edited archive
  can't fail the CHECK). It derives when the field is absent, not by
  format number. The insert's `ON CONFLICT` branch (an id named twice,
  only in a damaged archive) keeps the stored kind, so it sets the state
  against that kind.
- **Tasks don't repeat (S5b, format 12)**: a task carries no
  `recurrence` and the archive no `recurrence_links`. Formats 3 to 11
  may hold both. Neither has a field any more, so serde reads past them
  and a task that repeated restores as a one-time task (migration 053's
  rule). Don't give either a field again "to be safe": an unknown rule
  word then fails the whole restore
  (`an_older_archive_restores_its_repeating_tasks_as_one_time_tasks`).
- The trash (soft-deleted notes, tasks, captures) is not backed up, on
  purpose.
- Changing what an archive holds, or where, bumps
  `common::backup::SCHEMA_VERSION`, so older releases refuse it.
- **Task dependencies (S5d, format 13)** are
  `task_dependencies: Vec<TaskDependency>`, read by
  `repo::task_dependency::list_all_on` inside the snapshot: links between
  live tasks only, since the archive holds no trashed task. A plain
  `Vec`, like Briefs: a link hangs on its tasks, and every restore
  replaces the tasks. `restore_into` inserts them after the tasks, from
  `backup::restorable_dependencies` (`task_dependency::restorable`): a
  link needs both its tasks restored (a task with no node becomes a
  capture, so it is not one), a pair comes back once, and no loop comes
  back. The preview calls the same function, so its count is what the
  restore does.
- **S5c did not bump it** (it stayed 12): an archive holds the same things in
  the same places, only never a task without a node, so the release
  before can restore one made after. A rule that turns old content into
  something else on restore (a task with no node into a capture) reads the
  content itself, never the format number: a format-12 archive made by
  3.24 can still hold such a task
  (`a_restore_turns_a_task_with_no_node_into_a_capture`).

## Project states (S6a, #320)

- A Project is active, parked, complete or cancelled:
  `nodes.project_state` (TEXT with a CHECK, not an enum type: an enum
  value can't be dropped) and `project_state_at` (migration 051). The
  rules are `common::project_state`.
- **A Project always has a state and nothing else has one**
  (`nodes_project_state_only_on_projects`). Every `INSERT INTO nodes`
  supplies it: `insert_node` (active for a project), the restore, the
  `insert_raw_node` fixture in `pg_tests`, `scripts/perf/seed.sql`. A new
  one that forgets fails on the CHECK for every project, which is the
  point: it can't quietly re-open closed projects.
- **Every statement that fills a `NodeRow` names its columns through
  `node_cols(alias)`** (repo/node.rs). `FromRow` finds a missing column
  only when that route runs, so don't write the list out.
- **The state is declared, never derived.** Only migration 051 and the
  restore of an older archive read the label, once
  (`state_from_label`; a pg test holds the migration's SQL to it). No
  label gives "parked". Don't add a path that sets the state from the
  label, the tasks or the sub-projects (§4.1).
- **The only writer is `PUT /nodes/{id}/project-state`**
  (`NodeRepo::set_project_state`). Order: rights (`require_editor`),
  then the kind (`Capability::ProjectStatus`, 422), then the repo. In one
  transaction it locks the row, sets the state, stamps
  `project_state_at` when the state changes or was never declared,
  writes `project_state_changed` to History when it changed, and on a
  terminal state calls `project_focus::remove_on` for the **caller's
  own** entry. A repeat of a declared state writes nothing: `updated_at`
  is the version an open editor saves against. It returns whether it
  wrote the node, and the route fires `node.updated` only then. The date
  is `clock_timestamp()`, as History's is: `now()` is the transaction's
  start, before the wait for the lock.
  `two_declarations_at_once_apply_in_order` fails without the lock.
  `no_other_node_write_touches_the_project_state` holds `update` and
  `set_pinned` to that.
- `project_state_at` is "when a person last declared it". NULL means
  nobody has (the migration's reading, or a new project's active).
  Never fill it in. Don't add a "declared by" column: who declared is in
  History, which the purge already rewrites.
- **Who asks for attention is one rule**:
  `ProjectState::asks_for_attention` (active only). Focus reads it:
  `focus_refusal` / `holds_focus` in Rust, and `shows()` builds its SQL
  from the same method. So does a task: see "Task rules".
- **The dashboard shows what is not closed**: active and parked. It asks
  the node list with `exclude_closed`, whose SQL (`not_closed`) is built
  from `ProjectState::is_terminal`, and runs in SQL so closed projects
  don't use up its page. `NodeListParams.project_state` is the public
  filter. Don't write a state's name into a query: build it from the
  enum, as those two do.
- A duplicate is a new, active project: `copied_status` drops a label
  that reads as another state.
- Data health's `LabelReadsAsAnotherState`: the reader's **own**
  projects with `project_state_at IS NULL` whose label
  `label_disagrees` with their state. The reading is done in Rust, on
  one account's rows; the admin count groups labels in SQL and
  classifies them in Rust. A label is free text: never log one, and
  never read another account's into a report.
- Closing a project also closes what is under it: see "The cascade".

## The cascade (S6b, S6c, #326)

- **One rule, one function**: `common::cascade::plan`. The preview
  (`GET /nodes/{id}/project-state/cascade`) and the write both call it on
  rows read by `read_subtree` (repo/node.rs). Don't decide what closes
  anywhere else, in SQL or in the UI.
- **Rights are the actor's own grants**: `repo::editable_by(node, param)`,
  an owner or editor row in `permissions`. It has no admin branch, on
  purpose (§4.1, §14): never pass the cascade a "sees all". `viewable`
  binds the actor's subject to `visible_to`; never a NULL.
  A child the actor can't edit is skipped and **not walked into**; it is
  in the plan (`skipped`: "stays open") only when they can view it and
  it is not already closed.
- **The token is a hash of what the plan shows**
  (`CascadePlan::fingerprint`: the master, the state it has, the target,
  the ids that close and the ids that are skipped). The master's state is
  in it so that a catch-up confirmed from a stale page can't close a
  project that was re-opened meanwhile; `CascadePlan::is_catch_up` is the
  one reading of "already in this state". It is not a secret and grants nothing:
  the write decides from the rows under its own locks. A hidden child is
  in neither the plan nor the token. A rename changes neither.
- **When the PUT needs it**: a *change* to complete or cancelled whose
  plan closes something. Without a token, or with another plan's: 412
  (`PreconditionFailed`), nothing written. A plan that closes nothing
  needs none. The same terminal state again: without a token a no-op,
  as before (Data health's "Keep" sends it); with the plan's token it is
  the catch-up. Rights and the kind are checked before any 412.
- **Lock order, for anything that locks many node or task rows**:
  `SET LOCAL lock_timeout`, then `edge::lock_type_raw(Contains)`, then
  node rows `ORDER BY id` (`FOR NO KEY UPDATE`), then task rows
  `ORDER BY id`. The purge (nodes, then tasks) and `TaskRepo::reorder`
  (`a_reorder_takes_its_rows_in_id_order`) take theirs in id order for
  the same reason; a new bulk writer does too. A deadlock (40P01) is
  retried once (`a_cascade_that_loses_a_deadlock_goes_again`); a lock
  timeout (55P03), on a row or on the tree lock, is a 409 "try again"
  (`a_state_write_gives_up_on_a_row_held_too_long`). The timeout is per
  wait, not for the whole write. Keep the driver's error
  (`StateError::Db`) on any statement in `declare_once` that can wait
  for a lock: mapped to `Internal` first, it answers 500.
  A change to active or parked reads and locks the master's row only
  (`read_subtree`'s `walk`).
  The pg tests `two_confirmed_closes_at_once_cascade_once`,
  `a_close_and_a_childs_change_never_interleave`,
  `a_focus_add_never_lands_beside_a_cascade` and
  `a_cascade_runs_beside_a_reorder_and_a_purge` run it beside the other
  writers. They pass without the row locks too (checked 2026-11-14): the
  `contains` lock orders state writes, and the Focus add waits on the
  cascade's own `UPDATE`. The row locks are there so that a task or
  project changed meanwhile makes the cascade wait and then see it,
  rather than record something it did not do; `written_all` drops the
  transaction (412) if a statement still changes fewer rows than planned.
- **In the cascade, the state `UPDATE` comes before `remove_on`.** A
  Focus add holds `FOR SHARE` on the node; the update waits for it, and
  the delete that follows then sees the new entry. The other way round,
  an entry could land on a closed project.
- Tasks are cancelled by `task::cancel_open_on`, through
  `COMPLETED_AT_AFTER`: **no completion date** (owner decision,
  2026-10-30). "Still to do" is `task::still_to_do()`, from the enum.
- **History**: `cascade_closed` on every node where something closed
  (`common::cascade::recorded`), and each closed sub-project's own
  `project_state_changed`. The entry stores other nodes **by id only**
  (`sub_projects`, `root`). Anything that returns History entries runs
  them through `routes::nodes::present_for`, which resolves ids to titles
  for nodes this reader can view (`NodeRepo::visible_titles`) and drops
  the rest, uncounted: no empty list, and an entry left with no master,
  no sub-project and no task is not shown to that reader at all (alone
  it would say that something hidden was closed). A new place that shows
  History must call it; a
  title stored in the entry would be read by everyone who can view the
  node. Task titles are stored: a task is read by whoever can view its
  node.
- **Focus**: the cascade removes the actor's own entries on what it
  closes (`project_focus::remove_on`), nobody else's, and records none.
- **Webhooks** after commit, each by its event's existing rule:
  `node.updated` per project whose state changed, to that project's
  owner's hooks; `task.updated` per project whose tasks were cancelled,
  to the hooks of those tasks' owners (once per owner), and only to an
  owner who can still see the project (`dispatch_task_event`).
- **Re-opening (S6c)** is `common::cascade::reopen_refused`: under a
  closed home that is a Project the actor can view, no change to active,
  and none from a closed state to parked (422, `MASTER_IS_CLOSED`, names
  no node). Active to parked is allowed. A hidden master refuses
  nothing. `sees_all` (pass `is_admin(&claims)`) is for this check only:
  an admin reads every node, so is held to the rule; it never widens
  what the cascade closes. Re-opening a master changes the master alone.
- Data health's `OpenUnderClosedProject`: the reader's **own** closed
  projects with tasks still to do, or with direct sub-projects that are
  not closed and that the reader can view. `can_close` is `editable_by`,
  the cascade's rule, applied **one level down**: the plan goes all the
  way, so a project whose only closable leftover is deeper says
  `can_close: false` here; the deeper project is its own owner's item.
  The admin view gets the count.

## History and notes (S6d, #329)

- **History is notes plus the log's `is_history` actions**
  (`ActivityAction::is_history`: status label, state, cascade, replaced
  Brief, lifecycle). Everything else in `activity_log` is the audit
  trail. The match is exhaustive: a new action does not compile until it
  is put on one side. `ActivityScope` is the two sides; its `db_strs()`
  is the SQL filter.
- `GET /nodes/{id}/activity?scope=history|audit`: rights first
  (`require_viewer`), then the scope is parsed (422 for anything else),
  so a stranger gets 403 whatever they send. The filter runs in SQL
  before the `LIMIT` (#208's lesson: a node's few History rows must not
  be crowded out). `present_for` and `redact_emails` apply in every
  scope. No scope means everything: older tabs ask that way.
- The route reads one row more than asked and sets
  `common::activity::MORE_HEADER` (`x-ember-more: 1`) when the node has
  more. The page must not infer that from the count: `present_for` drops
  an entry its reader may not see **after** the limit, so a cut read can
  come back short.
- **The chronology is merged in `common::history::merge`**, by the page,
  from the two reads. There is no `/history` route on purpose: rights and
  redaction stay in the one route that has them, and the notes still load
  when the changes don't. A server route that needs the merged list calls
  the same function.
- **A note's `created_at` is never written after insert** (§9.2: its
  place in the chronology). `edited_at` is stamped by `NoteRepo::update`
  when `body IS DISTINCT FROM` the new text, and by nothing else: not a
  colour, not a delete or restore. Nothing a person reads may use a
  note's `updated_at`, which every row write moves (the trigger). The
  feed's `sort=updated` orders by `COALESCE(edited_at, created_at)`.
- **Notes read for a person are `ReaderNote`**: the note plus `may_edit`
  and `may_remove`, computed by `reader_flags` from the same SQL text
  the writes check (`author_may_write`, `may_remove`). Bind the reader's
  own subject, never a list scope: an admin's scope is NULL, and NULL
  wrote nothing. `the_note_flags_are_the_write_rules` holds each flag to
  whether the write succeeds. `Note` (no flags) is what the backup reads
  and the write routes return.
- Every statement that fills a `NoteRow` names its columns through
  `note_cols(alias)`.
- **Lifecycle** (draft, published, archived): `PgNodeRepo::update` is the
  only writer of `nodes.status` and logs `lifecycle_changed` (from, to)
  in its transaction when it changed. A new writer of that column must
  log it too. A save that **sends** only the status label or the
  lifecycle logs no `edited` line (`is_history_only`): the status chip,
  or an API client's bare publish. The app's editor sends the title and
  the body with the lifecycle, so a publish from it is an `edited` line
  (coalesced as any edit) and a `lifecycle_changed` one.
- **Legacy "edited" marks**: `common::note::legacy_edited_at` is the one
  rule (the row was last written three whole seconds or more after it was
  created), applied by migration 052 to live notes and by a restore of an
  archive from before format 11. `BackupNote.edited_at` is a double
  option: absent (older archive, the rule decides), null (never edited),
  or the date. Read it through `restored_edited_at()`.
- Writing, editing or deleting a note is not logged in `activity_log`
  and fires no webhook: the note is the record, and its text stays in
  one table.

## Focus (#282, proposal B)

- A person's Focus is `project_focus(subject_id, node_id, position,
  why_now, added_at)` (migration 048). The rules are
  `common::project_focus`; the module is named `project_focus` because
  `focus_date` is My Day.
- **Private, no admin bypass**, like captures and people. `FocusRepo`
  takes the person as `&str` and every statement has `subject_id = $1`.
  No route takes an account. Only `list_all` (the backup) crosses
  accounts. Route tests use `focus_admin()`: the stub panics when another
  account's Focus is asked for.
- `sees_all` (an admin) widens which of the reader's **own** entries show,
  because an admin may read every node. It never widens whose entries are
  read. Pass `is_admin(&claims)`, nothing else.
- **The read is a filter and deletes nothing.** `shows()` in
  `repo/project_focus.rs` leaves out a node that is not a Project, is
  archived, is not active (S6a: parked, complete or cancelled), or that
  the reader can't see (`visible_to`), in the same statement that reads
  the title. Its kind-and-state half is
  `common::project_focus::holds_focus` (`focus_refusal` gives the
  reason); `the_focus_filter_is_the_shared_rule` holds the two together.
  Anything new that leaves Focus changes both.
- A row is deleted only by its person (`remove`), its node (cascade), its
  account (the purge: `project_focus` is in `SETTINGS`), or by its person
  closing the project (`remove_on`, called from
  `NodeRepo::set_project_state`; "Project states" above).
- `PUT /focus/{node_id}` checks, in order: viewer rights
  (`or_missing(check, false, ..)`: hidden reads as missing, 404), then
  `focus_refusal` (422 with its reason: the kind, archived, parked,
  closed), then the why-line (422).
- **`FocusRepo::put` checks `focus_refusal` again under `FOR SHARE` on
  the node's row**, in the transaction that inserts. A state change
  holds `FOR NO KEY UPDATE`, so an add and a close never interleave:
  the add lands first and the close removes it, or the close lands first
  and the add is refused. Without it an add could slip in after a close
  and bring the project back to Focus on re-opening
  (`an_add_to_focus_never_lands_beside_a_close`).
  `why_now` is a double option (`FocusInput::change`): absent keeps the
  stored why-line, so an "add" from a stale page never wipes one.
- `GET`, `DELETE`, the order route and the why-line route never look at
  the node: their answers are the same for a foreign id, a missing one
  and an absent entry.
- **`PUT /focus/{node_id}/why` changes a why-line and never adds an
  entry** (`FocusRepo::set_why`; no row is a 404). The UI's why-line
  editor uses it, so a why-line saved from a page that is out of date
  can't put a removed project back in Focus. Anything else that edits an
  existing entry goes the same way, not through the upserting put.
- **`reorder` renumbers all of the person's entries**, 0 upwards. The
  ids it is sent swap among the slots they already hold; an entry it is
  not sent (one the list hides) keeps its slot. Don't number only the
  named ids: they land on top of the hidden ones
  (`a_reorder_keeps_the_place_of_an_entry_the_list_hides`).
- The why-line is one line with no control characters
  (`normalize_why`): a NUL can't be stored and would answer as a 500.
- **Never write Focus into History, a webhook or the node deletion
  preview.** All three are shared with the node, and each would tell its
  owner who has it in Focus.
- The cards of `GET /focus` come from `routes::tasks::dashboard_entries`,
  shared with the dashboard. Its caller must already have checked that
  the reader may view every node passed in.

## Server commands and automatic backups (2026-11-02)

- `api/src/cli.rs`: `ember-trove-api backup create [--comment TEXT]` runs
  instead of serving (parsed before anything connects; anything unknown is
  refused). It needs a shell on the server, never a sign-in: don't add an
  HTTP route or token for it.
- It records `common::backup::AUTOMATIC_ACTOR` as `created_by`, and then
  `backup::prune_automatic` keeps the newest `AUTOMATIC_KEPT` (10)
  automatic backups. An admin's backups are never pruned. Deleting a backup
  goes through `backup::delete_backup` (archive, then row), shared with the
  Delete route.
- It runs through `deploy/app-backup.sh` (public-log-safe output, full
  output in `~/ember-trove-backups.log` on the host): after each deploy
  (`release.yml`) and daily (`backup.yml`, sharing the deploy's
  concurrency group). Schedules live in Actions, never the host's
  crontab, so a failure is a red run and an email (the old `backup.sh`
  cron failed silently for seven months).

## "Visible to" in SQL (#281, 3.2)

- Two spellings only, both in `repo/mod.rs`:
  - `visible_to(alias, n)`: a per-row `EXISTS`. For a query that touches
    few nodes (a join from tasks or notes, a title lookup).
  - `visible_among_many(alias, n)`: `id IN (grants UNION own)`, hashed
    once. For a query that returns many of the caller's nodes (node list,
    title list, graph). Measured 2026-11-10 on 20,000 nodes: about twice
    as fast as `visible_to` there.
- A query scoped to `owner_id` alone is owned-only on purpose (graph edges
  and positions, search, the notes feed). Don't "fix" it to visible.
- `the_node_list_the_title_list_and_the_graph_agree_on_who_sees_what`
  holds the three big lists to one answer.
- One line of Markdown as plain text is `common::note::plain_line`
  (3.4); `markdown::plain_preview` and `note_title` both use it.

## Wiki links (#281, 3.1)

- `common::wikilink::wikilinks(text)` is the only scanner for `[[…]]`.
  It gives each link's byte range, target and display, and skips code
  (`code_ranges`). Edge sync, title resolution, the renderer, excerpts,
  note previews and "link this mention" all go through it or
  `replace_wikilinks`. Never write another `find("[[")` loop: the five
  that existed disagreed on `[[]]`, `[[x|]]`, trimming and unclosed
  openers.
- A rule change there changes which `wiki_link` edges exist: sync runs on
  the next save of each node.

## Request validation (#281, 1.5)

- An update request validates like its create: `UpdateNodeRequest` and
  `UpdateTaskRequest` derive `garde::Validate`, and the route calls
  `validate()` before anything else. A rule a create has (title length,
  `not_blank`, the 1 MiB body cap) belongs on the update too, or the
  database's CHECK answers it as a 500.
- A node's body snapshot goes through `snapshot_body` (routes/nodes.rs),
  awaited. Never `tokio::spawn` a version write: it can land after the
  next save's.

## Task rules (#281)

- **States (S5a, #314; ONTOLOGY §9.3): open, waiting, done, cancelled.**
  `in_progress` is gone: migration 050 turned it into open, and
  `node_tasks_no_in_progress` keeps it out (Postgres can't drop an enum
  value). `TaskStatus::parse` (the database's words) rejects it; serde
  reads it as open, for old archives and clients.
- **`completed_at` is set only by the statement that sets the status**
  (`COMPLETED_AT_AFTER` in repo/task.rs): now when a task becomes done,
  cleared when it leaves done, untouched otherwise. Any new write to
  `status` uses it. A task completed before migration 050 has none: never
  back-fill one.
- The database checks only "a date means done"
  (`node_tasks_completed_only_when_done`). Don't add "done means a date",
  even `NOT VALID`: Postgres applies a NOT VALID check to every update of
  an old row, so no task completed before 050 could be renamed, reordered
  or deleted again (`migration_050_…` holds this).
- **Only an Open task is actionable** (`TaskStatus::is_actionable`; owner
  decision, 2026-11-13). My Day carries over, and lists as overdue, only
  actionable tasks: `Task::in_my_day` and `Task::in_overdue_zone` are the
  rules, and `list_my_day`'s SQL is the first one
  (`my_day_carries_over_only_what_can_be_acted_on` holds them together).
  `is_overdue` stays the plain fact (past due and not finished), for the
  red date on a row.
- A query that buckets tasks by state names all four, or says "not
  terminal". The dashboard's counts and its list (`counts_for_nodes`,
  `list_open_for_nodes`: open before waiting) are the two that name them.

- **A task has exactly one Project** (S5c, #335; ONTOLOGY §9.3).
  - `node_tasks.node_id` is `NOT NULL` (migration 054, which turned the
    tasks with no node into captures; `capture::nodeless_task_text` is
    the same rule in Rust and `migration_054_…` holds the two together).
  - `POST /tasks` refuses a body that names no node, in the handler
    (`CreateTaskRequest` is shared with `POST /nodes/{id}/tasks`, whose
    body names none). `PATCH /tasks/{id}` refuses `node_id: null` in
    `UpdateTaskRequest::validate`. Both are 422s decided from the request
    alone, before any lookup, so they say nothing about a task: don't
    move either check after the fetch
    (`a_task_cannot_be_taken_off_its_project_and_the_refusal_names_no_task`).
  - `TaskRepo::create` and `insert_task` take a `NodeId`; `update` writes
    `COALESCE($9, node_id)`, so nothing asks the database for a NULL.
  - "The node is a Project" is still the API's rule only
    (`Capability::Tasks`, with the exemption for a task that stays where
    it is). There is no database rule for the kind: a restore of an older
    archive may still land a task on an Area or a Reference, and Data
    health lists it (`task_on_non_project`). A test may still put a task
    on a non-Project through the repo or raw SQL; it can't make one with
    no node.
  - Moving a task needs editor rights on both Projects. An editor of a
    shared Project can therefore move a task to a Project only they see
    (noted for the owner on #204, 2026-11-16; not changed).
- Who may change a task is `require_task_editor` (routes/tasks.rs): an
  editor of its node; an admin always. `TaskRepo::reorder` applies the
  same rule in SQL and skips the rest. There is no other case: until S5c
  a task on no node was its owner's to change.
- **`Task::node_id` is a `NodeId`, not an `Option`** (S5c). An archive's
  tasks are `common::backup::BackupTask`, the one type that still allows
  none; it writes word for word what `Task` writes
  (`an_archived_task_is_written_exactly_as_the_task_itself`), which is
  why the format stays 12. A new field on `Task` goes on `BackupTask`
  too, in the same place, or that test fails. A feed's `LEFT JOIN nodes`
  with `ON_A_NODE_THEY_SEE` now means "its node is one the reader sees",
  nothing else.
- A list's admin scope is `subject_scope(&claims)`, never an inline
  `if is_admin`. A task list's priority order is `PRIORITY_RANK` /
  `T_PRIORITY_RANK` (repo/task.rs). "Overdue" and "in My Day" are
  `common::task::is_overdue` and `Task::in_my_day`.
- **A task happens once** (S5b, #332; ONTOLOGY §9). Migration 053
  dropped `recurrence` and `recurrence_parent_id`. Completing a task
  writes that task and nothing else; reopening one deletes nothing.
  `update_task` is: validate, the rights checks, the update, the webhook.
  Recurrence comes back only as a new owner decision.
  - A tab from before S5b still sends `recurrence`, and `today` with a
    completion. No request type refuses unknown keys, so both are read
    past (`a_repeat_rule_from_an_older_tab_is_ignored_not_refused`).
    Don't add `deny_unknown_fields` to a task request.
- **A task asks for attention only when it is open and its project is
  active** (S5a, S6a): `Task::asks_for_attention(project)`. My Day's
  carry-over (`Task::in_my_day`) and its Overdue zone
  (`Task::in_overdue_zone`) take the project's state as a **required**
  argument, so no caller can decide without it; a task one place counted
  out of Today and another out of the Backlog would be in neither.
  `list_my_day` builds its SQL from the same two rules
  (`repo::task::asks_for_attention`), and
  `my_day_carries_over_only_what_asks_for_attention` compares the list
  with the rule for every task state, focus day and project state.
- `done_toggle` takes the project's state as well: completing re-stamps
  an earlier `focus_date` to today only for a task that was carried into
  Today (`asks_for_attention`). A waiting task, or one of a parked
  project, ticked in the Backlog stays where it was.
- The three feeds (My Day, Backlog, Calendar) read one column list,
  `feed_cols(n)`, and carry `project_state` from the **same** join as
  `node_title`: the one that checks the reader may see the node. Never
  add a second join to `nodes` in a feed.
- Never use the server's UTC date for a user-facing day when the client
  can say. `done_toggle` stamps `focus_date` with the viewer's own
  `today`, and the My Day and calendar reads take their day from the
  request.

## Task dependencies (S5d, #338)

- "Task A waits for task B" is a row of `task_dependencies(task_id,
  depends_on_id, created_at)` (migration 055). The rules are
  `common::task_dependency`; the SQL is `repo::task_dependency`. The
  table has **no account column**: a link follows its two tasks
  (`ON DELETE CASCADE` both ways), so the account purge has no step for
  it. Don't add `created_by`: the purge test sweeps such columns.
- **Surfaced, never enforced** (ONTOLOGY §6.4, §12). A blocked task is in
  every list it was in, and can be completed. Don't make `blocked_by`
  part of `asks_for_attention`, the feeds' `WHERE`, or any order.
- **Shown only through tasks the reader sees.** `shown_to(task, node, $n)`
  is the one spelling: the other task is live (not in the trash) and on a
  node the reader can view. `blocked_by_count(task_id, $n)` is the one
  spelling of "how many prerequisites hold this back": `shown_to` plus
  `holds_back_sql`, which is built from `task_dependency::holds_back`
  (everything but Done; a Cancelled prerequisite blocks for good).
  The two lists, the remove, `not_shown`, the node's task list and the
  three feeds all use them
  (`the_blocked_count_is_one_rule_on_every_task_list`). Anything new that
  shows or counts a dependency does too.
- The count rides on `ListedTask` (the node's list) and `MyDayTask` (the
  feeds), **never on `Task`**: it is per reader and not stored, and a
  `Task` is also what a write answers with and what a backup reads. In a
  feed it is a correlated subquery with its own `visible_to`: a
  prerequisite sits on its own node, so this is not the "second join to
  nodes" the feeds forbid. `feed_cols(n)` takes the placeholder the feed
  binds its reader at.
- `POST /tasks/{id}/dependencies` checks, in order:
  1. a task named as its own prerequisite (`refused_outright`, 422, from
     the request alone);
  2. the task, then `require_task_editor`;
  3. the other task: `or_missing_row(TASK)`, then `can_view` on its node,
     and a hidden one is `missing(TASK)`: the same 404 as one that doesn't
     exist. It is never looked at before step 2 has passed;
  4. the repo: the limit, the loop check, the insert.
  `a_dependency_is_added_only_by_an_editor_onto_a_task_they_can_see` and
  `a_prerequisite_the_caller_cannot_see_reads_exactly_like_a_missing_one`
  hold the order; the stub panics on a write that got past a check.
- `DELETE /tasks/{id}/dependencies/{other}` checks the task and
  `require_task_editor`, and `other` **only inside the delete**: it
  removes only a link whose other task the caller is shown. No link, a
  link to a trashed task and a link to a hidden one are the same 404, so
  the route can't be used to test an id.
- **A link nobody is shown must not be left behind** (security review,
  2026-11-16). It still decides which dependencies are refused as loops:
  an editor of a shared Project could tie two of its tasks together
  through a task of their own, for good. So `TaskDependencies.not_shown`
  tells someone who **may edit** the task how many of its prerequisites
  they aren't shown, never which, and `DELETE /tasks/{id}/dependencies/not-shown`
  removes them. "Not shown" is `not_shown_sql`: `NOT shown_to` the
  reader **and** not shown to the owner of the task's node either (in
  the trash, or on a node that owner can't view). The owner half came
  with #349: see "Links not shown". A viewer gets 0 and the count is never read for them
  (the stub panics). It is the hidden-home rule (#228).
  `a_chain_planted_through_a_hidden_task_can_be_found_and_removed` holds
  it. They never count as blocking.
- **The loop check looks at every link**, trashed and hidden included
  (`closes_loop`, the edges' CTE with `UNION`), inside a transaction that
  holds `pg_advisory_xact_lock(hashtext('ember.task_dependencies'))`.
  Without the lock two inserts close a loop together:
  `two_dependencies_added_at_once_cannot_close_a_loop` fails with it
  removed (checked 2026-11-16). A refusal names no task. It can tell
  someone who may edit one task and view another that a chain runs
  between them; node edges tell the same.
- **The limit (`MAX_PREREQUISITES`, 50) counts every row of the task.** A
  count of the shown ones alone could be stepped round through the trash
  (trash fifty, add fifty, restore). It tells an editor nothing more
  than `not_shown` already does.
- `add` sets `lock_timeout` (2 s) and `statement_timeout` (5 s) before it
  takes the lock, and answers 409 "try again" on either, or a deadlock
  (`an_add_that_cannot_have_the_lock_in_time_says_try_again`). The lock
  is one for every account, and a waiter holds a pool connection.
- `add` does not lock the two task rows. A task trashed in the same
  instant leaves an inert link (the reads leave it out); one deleted for
  good fails the foreign key, mapped to the route's 404.
- **The picker (`…/dependencies/candidates`) is bound to the person**:
  the repo takes `subject: &str`, never `Option`, and the route passes
  `claims.sub`. An admin's picker is not widened, unlike `/nodes/lookup`.
  It reads every task still to do, so it spells visibility as
  `visible_among_many`. It offers tasks still to do, not on an archived
  node, not already a prerequisite, and never one `add` would refuse as a loop: its `waits`
  CTE walks the same links as `closes_loop`, the other way
  (`the_dependency_picker_offers_only_what_can_be_added`). `q` goes
  through `escape_like`.
- Add and remove answer with the fresh `TaskDependencies`. `may_edit` is
  `require_task_editor`'s rule read for the reader, so the page offers a
  control only where the call would pass. **No webhook**: the task
  itself is unchanged.
- **Whose view a call is made for is tested**: the route tests' stub
  records the `viewer` it was given (`assert_all_seen_as`). An ordinary
  account is `Some(sub)`, an admin `None`. A route that passed `None`
  for everyone would show and remove hidden links with every status code
  unchanged. A new dependency read or write joins those assertions.
- **Not counted in the node deletion preview** (2026-11-16): a link to a
  task on another node goes with a deleted node's tasks, uncounted. A
  count would have to be worked out per reader to stay inside what they
  can see. (S7a's `still_used_by` shows how such a count is done; this
  one is still not built.)

## Node deletion preview (#281)

- `GET /nodes/deletion-preview?ids=a,b` feeds the delete confirmation.
  Each id passes `require_owner`, the check `DELETE /nodes/:id` makes,
  before anything is counted. A node that fails it is left out, the same
  as one that doesn't exist: only a 403 means "not yours", any other error
  propagates. The answer is counts only (`NodeDeletionPreview`).
- `NodeRepo::deletion_preview` counts the rows the `nodes` cascade
  removes. A new table that holds a node's content, and that a person
  would miss, joins that query and `NodeDeletionPreview::losses`.
- **`still_used_by` is the one count there that is not a loss** (S7a,
  §6.3): `references` links into the nodes from live nodes that stay.
  It has its own line (`NodeDeletionPreview::warning`), never
  `losses()`. It is the one count read **per reader**: the query takes
  `viewer` (`subject_scope`) beside `caller`, and counts a link only
  when its source passes `visible_to` (§14: a warning may understate,
  never reveal). It leaves out links from nodes in `mine`, the ids that
  passed `require_owner`, not the ids as sent. It counts links, so the
  UI's batches add up; a link from a node deleted in another batch of a
  bulk delete is counted (it overstates, and still never counts a
  hidden node). A new per-reader count there takes `viewer` the same
  way.

## Reverse indexes (S7a, #342)

- **One answer, grouped in the page.** `GET /nodes/{id}/edges`
  (`EdgeRepo::list_for_node_with_titles`) holds only links whose two
  ends the reader can view, so every group, count and notice built from
  it is cut to what they see. `common::connections` reads it:
  `group_of(edge_type, outgoing)` is an exhaustive match (every type and
  direction has exactly one group, so no link is left off the page),
  `grouped` orders it, `live_users` is the archive notice's count.
  Don't add a list route per group, and don't count in the browser from
  anything but this answer. (S7b's `GET /nodes/{id}/blockers` is not a
  group: it is what lies beyond the answer. Its server-side count is
  held equal to `blockers::direct` of this answer by a test.)
- **"Live"** is `common::node::is_live(status, project_state)`: not
  archived, and not a complete or cancelled Project. Not "active": that
  is a Project's state. Its SQL is `repo::node::live(alias)`, built from
  `live_words()`; `the_sql_for_a_live_node_is_the_shared_rule` holds
  them together.
- **Who may remove a link is `EdgeType::removable_by(edits_source,
  edits_target)`**, one rule for `DELETE /edges/{id}`,
  `LinkRef::removable` and `EdgeWithTitles::may_remove`. The row's flag
  binds the reader's own subject through `editable_by` (grants only; an
  admin is the NULL subject), as `require_editor` reads it.
  `a_nodes_links_say_what_each_end_is_and_who_may_remove_them` checks it
  for every type from both ends and five readers.
- **An Area is never a `derived_from` origin** (§6.5):
  `EdgeRefusal::DerivedFromArea` in `edge_kind_violation`, after the
  rights checks like every kind rule. Data health's `DerivedFromArea`
  lists the old ones (`derived_from_areas`); each item carries
  `filed: FiledRef {kind, has_home}`, and `FiledRef::fix()` is the fix
  that fits (`MakeHome`, `LinkAsSource`, `Remove`).
  `LinkRef::filing_fix_allowed` offers one only where its calls pass.
  `has_home` is said as `GET …/home` would say it: to an editor of the
  filed node any home counts, a hidden one included; to anyone else
  (the Area's owner, who is listed the item too) only a home they can
  see. `MakeHome` is applied with `POST /edges`, never the home route:
  that one replaces a home, and a stale "no home" must be refused.
- **`POST /edges/{id}/convert` with `reverse`** turns the link round in
  the transaction it already has (`EdgeRepo::convert(.., swap)`).
  `convert_ends(from, to, reverse, source, target)`: step 3 then needs
  edit rights on the old target and view rights on the old source; a
  `related_to` has no direction to turn. It is general: any link the
  caller could delete and then make the other way round.

## Blockers (S7b, #346)

- **Display only.** Nothing here writes, prevents or reorders (§6.4).
  The rules are `common::blockers`: `satisfies_dependency` (the §6.4
  table), `still_waits` (in use and not itself met), `is_blocked` (it
  still waits and has an unmet dependency: a Published Article is never
  "Blocked"), `passes_on` (a chain goes on only through a node that
  still waits, never an Area), `chain` (generic; tasks use it through
  `task_dependency::chained`).
- **The SQL twins are built from the rule's own triples**
  (`repo::node::satisfies_dependency`, `passes_on`, from
  `satisfying_triples()` / `passing_triples()`): an `IN` over whole
  `(kind, status, state)` rows with `COALESCE(project_state, '')`.
  Never `NOT (state IN (…))`: it is NULL for a node with no state and
  drops the row. `the_sql_for_a_met_dependency_and_for_going_on_are_the_shared_rules`
  holds them to the rule.
- **A walk's row is the link alone**, `(source_id, target_id)`, and its
  halves are joined with `UNION` (`EdgeRepo::blockers`,
  `TaskDependencyRepo::chain`). A depth or a path in the row makes every
  pass a new row, and a legacy loop (a restore keeps one) never ends.
  The cut (`LIMIT CHAIN_LIMIT + 1`, numbered with `row_number()`)
  comes before the titles are joined, and the link past the limit is the
  one dropped: it is the last found, so nothing kept hangs on it. Never
  trim by the order of the joined rows: that dropped a link from the
  middle of a long chain. The limit counts the node's own unmet
  dependencies too.
- **Every hop checks the reader, and so does the start.** The start's
  own visibility is in the SQL of all four reads (`blockers`,
  `blocked_by_for_nodes`, `chain`, `blocked_tasks_for_nodes`), though
  every route checks it first: a count of links from a hidden node is a
  link with a hidden end. A link is taken when its target is
  `visible_to` the reader (tasks: `shown_to`) and not met. The walk
  stops at a hidden node and shows nothing beyond it, even a node the
  reader could read: "stops here" and "nothing further" must be one
  answer. `a_chain_of_blockers_is_walked_only_through_what_its_reader_can_see`
  checks that every node of a chain hangs on a link the reader finds in
  that hop's own `list_for_node_with_titles`.
- **A per-reader count never goes into `Node`.** `GET /nodes/{id}`
  answers `ViewedNode { node, blocked_by }` (the `ListedTask`
  precedent). `Node::edge_count` is the cautionary case: only `list`
  fills it, and the dashboard's internal `list` call runs with no
  subject.
- **The cards' counts** (`ProjectDashboardEntry::blocked_by`,
  `blocked_tasks`) are read in `dashboard_entries` with
  `subject_scope(claims)`, after its five other reads: a Focus card may
  be someone else's Project. `blocked_tasks_for_nodes` reuses
  `blocked_by_count`; the caller has checked the nodes.

## Links not shown (#349, ONTOLOGY §14)

- A link's source needs an editor and its target only a viewer, so an
  editor of a shared node can link it to a node its owner can't see; a
  share ending hides a link too. Nobody lists such a link for the owner,
  and it still decides loop refusals and reads as a blocker to whoever
  sees both ends (owner decision 2026-11-17: it stays a real link until
  removed).
- **`EdgeRepo::not_shown(node, viewer)` and `remove_not_shown`** share
  one predicate (`not_shown_predicate`): the link is one
  `EdgeType::is_its_nodes_to_clear(outgoing)` accepts, the reader holds a
  grant to edit the node (`editable_by`), and the other end is visible
  **neither to the reader** (`visible_to`) **nor to the node's owner**
  (`owned_or_granted("o", "n.owner_id")`).
- **The owner half is the security of it.** "Hidden from the reader"
  alone tells an editor of a shared node how many private nodes its
  owner links it to, and gives them one statement that deletes those
  links. `an_editor_is_not_told_of_and_cannot_clear_the_owners_private_links`
  holds it. The task rule shipped without it in 3.26.0; `not_shown_sql`
  in `repo/task_dependency.rs` has it now
  (`an_editor_is_not_told_of_the_owners_private_prerequisites`).
- **The owner is a column in the SQL**, never a bound value: bound as
  an `Option`, a NULL reads as "sees everything" in `visible_to`. Use
  `owned_or_granted(alias, "n.owner_id")`.
- **Which links** is `EdgeType::is_its_nodes_to_clear`, derived from
  `removable_by`: what the node's editor could remove if shown it. Not
  an incoming `contains` (the Home row), not another node's link into
  this one, never a `wiki_link` (counting it would let `[[a guess]]`
  probe titles). The SQL's type lists come from
  `EdgeType::a_nodes_to_clear`; don't write the names in a query.
- **The remove is one statement that applies the predicate itself.**
  Never delete ids taken from an earlier count: a link that became
  visible meanwhile must stay. No lock: taking a link away can't make a
  loop or a second home.
- **Routes.** `GET /nodes/{id}` fills `ViewedNode.links_not_shown` only
  when `require_editor` passes (else 0, and the repo isn't asked).
  `DELETE /nodes/{id}/edges/not-shown` calls `require_editor` before
  anything is read, so a foreign and a missing id answer alike (403, as
  node routes do). Both pass `subject_scope(&claims)`: an admin is shown
  every link, so their count is 0 and their remove takes nothing.
- **Never write it into History, a webhook or the activity log**: each
  would tell the node's viewers the count. The route logs the node id
  and the number on the server.

## Security rules from the 2026-11-10 audit

- **A note or task follows its node's rights, not its author's** (owner
  decision). `require_task_editor`: an editor of the task's node, never
  its owner as such. Since S5c a task can't be taken off its node either
  (422): that was one way to keep a task after a share ended. A move to
  another Project the editor may edit is the other, and is still allowed
  (the owner's to decide; see "Task rules"). Notes:
  `repo::note::author_may_write` (author **and** still an editor) for
  update, `may_remove` (that, or the node's owner, or an admin) for
  delete and restore; the routes pass a `NoteActor`. The owner's feeds
  (My Day, calendar, backlog, notes feed) leave out an item whose node
  the reader can't see (`ON_A_NODE_THEY_SEE`; the notes feed's inner
  join). Never let `owner_id = caller` alone open something that hangs
  on a node. Rights read the `permissions` table only: a pg test's node
  needs its creator's Owner row (`own` in `pg_tests`).
- **Only an admin's invite creates an account** (owner decision).
  `invite` answers a non-admin 422 (`NO_ACCOUNT_FOR_ADDRESS`) before
  anything is asked of Cognito beyond the lookup. Any new route that can
  reach `UserDirectory::create_user` needs `is_admin`.

- **A task event is sent with `dispatch_task_event`, never `dispatch`**
  (owner decision, 2026-11-16). It goes to the hooks of the task's
  author only while that person can still see the task's node
  (`NodeRepo::visible_titles`, the `visible_to` rule: they own it or hold
  a grant). A task follows its node's rights: someone whose share ended
  used to keep hearing about the tasks they wrote there. The author is
  read as an ordinary account (`Some(subject)`), so an admin's hooks hear
  only about nodes the admin owns or was given; a failed read sends
  nothing. The node `dispatch` **refuses** a task event
  (`common::webhook::is_task_event`), so a call site can't skip the
  question however it passes one (`the_node_dispatch_refuses_a_task_event`);
  the delivery itself is the private `fan_out`. Node events stay with
  `dispatch`, to the node's owner. `WebhookRepo::list_active_for_event`
  takes the owner and reads only their hooks. A task moved onto a node
  its author can't see is no longer told to them. "Sees" is the lists'
  rule (`visible_to`: owns it or holds a grant), not the routes'
  (`permissions` only); they differ for a creator whose Owner row an
  admin removed.

- **Every grant path calls `guard_canonical_owner`**: grant, update,
  revoke *and invite*. `PermissionRepo::grant` is an upsert, so any new
  route that ends in it can rewrite the creator's row.
- **A node's grants are listed with `list_visible`**, never `list`, in
  anything a non-owner can call. `list` is for checks inside a route that
  already required Owner.
- **History for a non-owner goes through `common::activity::redact_emails`.**
  New metadata that holds an address joins `EMAIL_KEYS`.
- **An id in the path gets its own ownership check** even when the route
  already checked the node: `attach_tag` loads the tag and answers a
  foreign one like a missing one.
- **A URL the app will open is `common::node_link::is_web_url`** (http or
  https from the first character). The forms send
  `with_web_scheme(typed)`.
- **Roles come from the token, so removing `admin` needs
  `auth::demoted`**: the route writes `admin_demotions`, then the
  in-memory `DemotedSet`, then asks Cognito. `validate_and_map` drops
  `admin` from a token issued at or before the demotion. The purge deletes
  the row; the daily sweep drops rows older than a day.
- **Webhooks**: create and update call `validate()`; at most
  `MAX_WEBHOOKS_PER_OWNER` per account. Each delivery runs in
  `webhook_dispatch::in_a_slot` (`MAX_IN_FLIGHT`). The client stays one
  per hook: it carries that hook's pinned DNS answer, and a shared client
  can't.
- **A route that fetches something by its own id answers a foreign id
  like a missing one**: `permissions::or_missing_row(WHAT)` on the fetch
  and `or_missing(check, may_see, WHAT)` on the rights check. `may_see`
  is whether the caller can already see the thing (`can_view` on its
  node, or it being their own grant): only then is a refusal a 403. The
  test `a_foreign_thing_reads_exactly_like_a_missing_one` lists every
  such route; a new one joins it.
- **A count shown beside a node counts only what its reader can see**:
  the node list's link count is `visible_link_counts`.
- **The edge proxy sets `X-Forwarded-For $remote_addr`**, never
  `$proxy_add_x_forwarded_for`: the API's limiter keys on that header.
  Tests in `routes/mod.rs` read `deploy/nginx.prod.conf` and hold this
  and the limits.
- **`release.yml` prints no API log lines**: its log is public.

## Admin permission model

- `require_role()` returns `Ok(())` immediately when `claims.roles.contains("admin")`.
- `list_nodes` skips the `subject_id` filter for admins.
- Ownership of a non-node resource is `require_resource_owner(claims, owner_id)`
  (auth/permissions.rs): the owner, or an admin. People, captures and
  memberships are the exceptions: owner only, no admin bypass.
- Admin sub: `f1eb2590-0091-70e4-d9b3-24e4a23d24d1` (`julian@chultarsky.com`).

## AWS SDK / TLS

- AWS SDK crates (`aws-sdk-{s3,cognitoidentityprovider,sesv2}`) use
  `default-features = false` + `default-https-client` (modern rustls 0.23 + aws-lc-rs).
  Do **not** re-enable the legacy `rustls` feature — it drags in the advisory-laden
  rustls 0.21 / rustls-webpki 0.101 chain that was deliberately eliminated (see
  `.cargo/audit.toml` history and `.claude/ERRORS.md`).
- AWS SDK requires rustc ≥ 1.91.1; the pinned toolchain (`rust-toolchain.toml`) clears it.

## Cognito Hosted UI

- `SetUICustomization` uses an allowlist — unlisted CSS classes raise
  `InvalidParameterException`. Authoritative CSS: `deploy/cognito.css` + `deploy/logo.png`.
- Apply via `boto3.client('cognito-idp').set_ui_customization(...)` (or the `aws` CLI with `--region us-east-2`).
- Pool: `us-east-2_4RQfxhKqn` · Client: `eogq2sehdad3uc8nmar7aneol`.
