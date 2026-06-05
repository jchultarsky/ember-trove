# Ember Trove — Roadmap & Architecture Decisions

Living document: current state, backlog, and the decisions behind the architecture.
Keep it current as part of each change (see `POLICY.md` §10).

## Current state (2026-06-05)

- **Released:** v2.19.2. The v2.19.x line closed the deep security review/audit
  (sprints 7–9): CSP nonce + `strict-dynamic` (removed `script-src 'unsafe-inline'`),
  Cognito admin hardening, activity-log scoping, backup/restore authz, rate-limiting
  coverage, full sqlx-parameterization sweep.
- **Toolchain:** Rust pinned to `1.96` (current stable) in `rust-toolchain.toml`;
  workspace edition 2024, resolver 2.
- **Pipeline:** `CI` (check/clippy/fmt/audit/migrations/coverage/docker-build) +
  `Release` (GHCR images, GitHub Release, EC2 deploy) on tag push.

## Backlog / candidate work

- **Wire webhook delivery.** `api/src/webhook_dispatch::dispatch` is built and
  tenant-scoped (SSRF-hardened registration already shipped) but has **no callers** —
  registered webhooks never fire. Wire `dispatch(...)` into the node/task mutation
  handlers (create/update/delete) so subscribers actually receive events, with tests.
  Surfaced 2026-06-05 when the crate-wide `#![allow(dead_code)]` was removed; the
  module now carries a scoped allow until this lands.
- Convert coverage from report-only to a `--fail-under` gate once the baseline settles.
- UI test strategy: more logic pushed into `common/` for host-target unit coverage;
  decide on a WASM/browser e2e harness.
- Revisit `cargo-deny` (licenses + bans + sources) as a superset of `cargo-audit` —
  deferred for now to avoid overlap.
- Optional: `lldb-dap` for editor step-debugging (not installed).

## Architecture decisions (ADR-lite)

- **Edition 2024 + resolver 2.** Latest stable edition; matches toolchain currency policy.
- **Modern AWS TLS stack.** `default-features = false` + `default-https-client`
  (rustls 0.23 + aws-lc-rs) on AWS SDK crates — eliminated the legacy rustls 0.21 advisory
  chain. Do not reintroduce the `rustls` feature. (2026-05-29)
- **`audit.toml` as single source of truth** for ignored RUSTSEC advisories; transitive-only,
  dated, reviewed per release.
- **Git Flow** (feature/release/hotfix). Heavyweight for a solo maintainer — even its author
  concedes trunk-based fits continuously-deployed web apps better — but the release/CD tooling
  (`next-version.sh`, tag-triggered `Release`) is built around it, so it stays. Reassess if/when
  contributor count or release cadence changes.
- **Default rustfmt, no `rustfmt.toml`.** Adopted 2026-06-05 with a one-time workspace
  reformat; enforced by hook + CI. Editors format with `--edition 2024`.
- **SHA-pinned GitHub Actions + Dependabot.** Supply-chain hardening consistent with the
  project's security posture; Dependabot keeps pins current.
