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
  rule). Restore also clears the live captures, nodeless tasks and people,
  which the `nodes` cascade never reached.
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

## What a backup holds (#281, format 5; Briefs made it 6, Focus 8, task states 9, project states 10)

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
- The trash (soft-deleted notes, tasks, captures) is not backed up, on
  purpose.
- Changing what an archive holds, or where, bumps
  `common::backup::SCHEMA_VERSION`, so older releases refuse it.

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
- Until the cascade exists (S6b, S6c), closing a project changes the
  project alone.

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

- Who may change a task is `require_task_editor` (routes/tasks.rs): on a
  node, an editor of that node; on none, its owner; an admin always.
  `TaskRepo::reorder` applies the same rule in SQL and skips the rest.
- A list's admin scope is `subject_scope(&claims)`, never an inline
  `if is_admin`. A task list's priority order is `PRIORITY_RANK` /
  `T_PRIORITY_RANK` (repo/task.rs). "Overdue" and "in My Day" are
  `common::task::is_overdue` and `Task::in_my_day`.
- `TaskRepo::create_occurrence` locks the parent row and returns the live
  occurrence it already has: a parent has at most one. Without the lock,
  `two_completions_at_once_schedule_one_next_occurrence` fails.
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
  `FEED_COLS`, and carry `project_state` from the **same** join as
  `node_title`: the one that checks the reader may see the node. Never
  add a second join to `nodes` in a feed.
- "Today" for a recurring task's next occurrence is the sender's:
  `UpdateTaskRequest.today`, read through `common::task::local_today`
  (within a day of UTC, else UTC). `done_toggle` sends it. Never use the
  server's UTC date for a user-facing day when the client can say.

## Node deletion preview (#281)

- `GET /nodes/deletion-preview?ids=a,b` feeds the delete confirmation.
  Each id passes `require_owner`, the check `DELETE /nodes/:id` makes,
  before anything is counted. A node that fails it is left out, the same
  as one that doesn't exist: only a 403 means "not yours", any other error
  propagates. The answer is counts only (`NodeDeletionPreview`).
- `NodeRepo::deletion_preview` counts the rows the `nodes` cascade
  removes. A new table that holds a node's content, and that a person
  would miss, joins that query and `NodeDeletionPreview::losses`.

## Security rules from the 2026-11-10 audit

- **A note or task follows its node's rights, not its author's** (owner
  decision). `require_task_editor`: on a node, an editor of the node; the
  task's owner passes only for a task on no node. Notes:
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
