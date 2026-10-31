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
  direction). Other types stay source-editor only.
- `related_to` is stored once per unordered pair (#226): the partial
  unique index `edges_related_to_once` on `LEAST/GREATEST(source, target)`
  answers a reverse insert with the same 409 as a duplicate. Restore folds
  pairs with `common::edge::fold_related_pairs`, migration 041's rule; the
  041 pg test asserts the two agree. Tests that need a legacy both-ways
  pair call `allow_legacy_related_pairs` (drops the index).
- The restore preview counts rule breaks with
  `common::data_health::graph_rule_counts` and warns; it never refuses
  (owner decision).

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
