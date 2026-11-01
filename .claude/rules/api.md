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
- Restore accepts formats 3 and 4 (`OLDEST_RESTORABLE`). A format-3 note
  without a node becomes a capture (`capture::from_nodeless_note`, 043's
  rule). Restore also clears the live captures and nodeless tasks, which
  the `nodes` cascade never reached.
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

## Admin permission model

- `require_role()` returns `Ok(())` immediately when `claims.roles.contains("admin")`.
- `list_nodes` skips the `subject_id` filter for admins.
- `is_owner`: `user.sub == n.owner_id || user.roles.contains("admin")`.
- Admin sub: `f1eb2590-0091-70e4-d9b3-24e4a23d24d1` (`julian@chultarsky.com`).

## AWS SDK / TLS

- AWS SDK crates (`aws-sdk-{s3,cognitoidentityprovider,sesv2}`) use
  `default-features = false` + `default-https-client` (modern rustls 0.23 + aws-lc-rs).
  Do **not** re-enable the legacy `rustls` feature — it drags in the advisory-laden
  rustls 0.21 / rustls-webpki 0.101 chain that was deliberately eliminated (see
  `.cargo/audit.toml` history and `.claude/ERRORS.md`).
- AWS SDK requires rustc ≥ 1.91.1; the pinned 1.96 toolchain clears it.

## Cognito Hosted UI

- `SetUICustomization` uses an allowlist — unlisted CSS classes raise
  `InvalidParameterException`. Authoritative CSS: `deploy/cognito.css` + `deploy/logo.png`.
- Apply via `boto3.client('cognito-idp').set_ui_customization(...)` (`aws` CLI unavailable).
- Pool: `us-east-2_4RQfxhKqn` · Client: `eogq2sehdad3uc8nmar7aneol`.
