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
- Restore accepts formats 3 to 6 (`OLDEST_RESTORABLE`). A format-3 note
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

## What a backup holds (#281, format 5; Briefs made it 6)

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
  selects `secret`, and `BackupWebhook` has no field for it. A webhook
  that signed is restored switched off (`BackupWebhook::restored_active`).
- **Project Briefs (#282, format 6)** are `briefs: Vec<ProjectBrief>`,
  read by `repo::brief::list_all_on` inside the snapshot. A plain `Vec`,
  not an `Option`: a Brief hangs on its node, the nodes are replaced by
  any restore, so an older archive can't leave live Briefs behind.
- The trash (soft-deleted notes, tasks, captures) is not backed up, on
  purpose.
- Changing what an archive holds, or where, bumps
  `common::backup::SCHEMA_VERSION`, so older releases refuse it.

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

- Who may change a task is `require_task_editor` (routes/tasks.rs): its
  owner, an admin, or an editor of its node. `TaskRepo::reorder` applies
  the same rule in SQL and skips the rest.
- A list's admin scope is `subject_scope(&claims)`, never an inline
  `if is_admin`. A task list's priority order is `PRIORITY_RANK` /
  `T_PRIORITY_RANK` (repo/task.rs). "Overdue" and "in My Day" are
  `common::task::is_overdue` and `Task::in_my_day`.
- `TaskRepo::create_occurrence` locks the parent row and returns the live
  occurrence it already has: a parent has at most one. Without the lock,
  `two_completions_at_once_schedule_one_next_occurrence` fails.
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
  `MAX_WEBHOOKS_PER_OWNER` per account.
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
