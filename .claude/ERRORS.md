# Known Errors & Fixes — self-learning log

Grep this **before** debugging a compile/runtime/CI error or inventing a new pattern.
Append new entries as you learn them. Format: **Symptom → Cause → Fix → date**.

---

## Rust / Cargo

### `cargo fmt` surfaces new clippy lints (e.g. `unnecessary_sort_by`)
- **Symptom:** clippy was green, you ran `cargo fmt`, and now `clippy -D warnings` fails
  (e.g. `unnecessary_sort_by` on `nodes.sort_by(|a, b| a.x.cmp(&b.x))`).
- **Cause:** rustfmt rewrites a block-body closure `|a, b| { expr }` into an
  expression-body closure `|a, b| expr`; some clippy lints only match the latter. fmt
  didn't break the code — it exposed a latent lint.
- **Fix:** always run clippy **after** fmt (the pre-commit hook does fmt+clippy in order).
  For this lint, prefer `sort_by_key(|n| key)` / `sort_by_key(|n| Reverse(key))`.
  Real fix: `ui/src/components/node_list.rs::sort_nodes`. — 2026-06-05

### AWS SDK pulls advisory-laden rustls 0.21 chain
- **Symptom:** `cargo audit` flags RUSTSEC-2026-0049/0098/0099/0104 (rustls 0.21 /
  rustls-webpki 0.101.7).
- **Cause:** AWS SDK crates' default/legacy `rustls` feature.
- **Fix:** set `default-features = false` and use `default-https-client` (rustls 0.23 +
  aws-lc-rs) on `aws-sdk-{s3,cognitoidentityprovider,sesv2}`. Verify with
  `cargo tree -i rustls-webpki@0.101.7` (must return nothing) + a live TLS round-trip.
  — 2026-05-29

### `cargo audit` ignore list diverged from CI
- **Symptom:** an advisory ignored locally still fails in CI (or vice-versa).
- **Cause:** inline `--ignore` flags in `ci.yml` *and* a list in `.cargo/audit.toml`.
- **Fix:** `.cargo/audit.toml` is the **single source of truth**; CI runs bare
  `cargo audit`. Never add inline `--ignore`. Each ignore is transitive-only and dated. — 2026-05-29

## CI / tooling

### `cargo fmt --check` fails across the whole repo
- **Symptom:** the new fmt gate fails on hundreds of pre-existing lines.
- **Cause:** the codebase was hand-formatted and never run through default rustfmt.
- **Fix:** one-time `cargo fmt --all` pass (committed as `style:` in isolation), then the
  gate is greenable. Editors must format with `--edition 2024` to match the workspace. — 2026-06-05

### CLAUDE.md referenced non-existent files
- **Symptom:** "grep `.claude/ERRORS.md` / `.claude/patterns/` before debugging" — but
  the `.claude/` tree didn't exist; every pointer was dead.
- **Cause:** docs described an intended structure that was never created.
- **Fix:** created the `.claude/` tree (this file, `POLICY.md`, `ROADMAP.md`, `rules/`,
  `patterns/`) so the references resolve. Keep docs and filesystem in sync. — 2026-06-05

## Leptos / WASM

### Reactivity silently stops working
- **Symptom:** a closure compiles but the view never updates; clippy/compiler may warn the
  closure is `FnOnce`.
- **Cause:** a non-`Copy` value (signal, `NavigateFn`) moved into an inner `move ||`,
  consuming it so the closure can only run once.
- **Fix:** clone the value before each inner closure, or wrap in `StoredValue` and use
  `get_value()`. See `.claude/patterns/navigate-reactive.rs`. — (carried from CLAUDE.md)

### `NavigateFn` won't go into a reactive context
- **Symptom:** `use_navigate()` result errors when captured by multiple closures.
- **Cause:** `NavigateFn` is `Clone`, not `Copy`.
- **Fix:** `let navigate = StoredValue::new(use_navigate());` then
  `navigate.get_value()("/path", Default::default())`. — (carried from CLAUDE.md)
