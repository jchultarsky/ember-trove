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
