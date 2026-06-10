# Ember Trove — Roadmap & Architecture Decisions

Living document: current state, backlog, and the decisions behind the architecture.
Keep it current as part of each change (see `POLICY.md` §10).

## Current state (2026-06-06)

- **Released:** v2.20.4. The v2.20.1–2.20.4 patches restored login, which the v2.20.0
  deploy had broken in two stacked ways: (1) the `/api/auth/callback` redirect used an
  inline `<script>` that the new strict CSP (nonce + `strict-dynamic`) blocked → blank
  page; fixed with a real HTTP 303 redirect. (2) `jsonwebtoken` 9→10 ships **no** built-in
  crypto backend, so RS256 session-token verification panicked → 502 on every authed
  request; fixed by enabling the `aws_lc_rs` backend (matches our rustls/AWS-SDK provider;
  avoids the `rsa` crate / RUSTSEC-2023-0071). Also: auth rate limits loosened for active
  dev (`/api/auth/me` carved into its own zone so the AuthGate can't loop), a pre-commit
  secret scan + "no private keys in tests" policy, and a fixed local Docker stack
  (`COOKIE_KEY` required from `.env.local`).
- **Prior (v2.19.x):** closed the deep security review/audit (sprints 7–9): CSP nonce +
  `strict-dynamic` (removed `script-src 'unsafe-inline'`), Cognito admin hardening,
  activity-log scoping, backup/restore authz, rate-limiting coverage, full
  sqlx-parameterization sweep.
- **Toolchain:** Rust pinned to `1.96` (current stable) in `rust-toolchain.toml`;
  workspace edition 2024, resolver 2.
- **Pipeline:** `CI` (check/clippy/fmt/audit/migrations/coverage/docker-build) +
  `Release` (GHCR images, GitHub Release, EC2 deploy) on tag push.

## Backlog / candidate work

- 2026-06-09 usability review: **fully shipped** (Tier 1: autosave,
  rollback sweep, undo-delete; Tier 2/3: skeletons + OS dark default,
  unlinked mentions, NL quick-add, inbox triage, palette commands, a11y pass,
  local graph + orphans lens). Remaining nice-to-haves, unscheduled: local graph on node pages
  + global-graph filters/orphans; unlinked mentions under backlinks; keyboard
  inbox-triage mode; NL quick-add parsing; palette actions beyond nodes;
  a11y pass (focus trap/return, route-change focus, color-only priority dots);
  skeletons in Notes/Search/Templates; `prefers-color-scheme` default.
- UI test strategy: more logic pushed into `common/` for host-target unit coverage;
  decide on a WASM/browser e2e harness. **Needs a direction** — this is an
  architecture choice (Playwright vs. `wasm-bindgen-test` headless vs. none), not a
  mechanical task; left for the maintainer to steer.
- Optional: `lldb-dap` for editor step-debugging (not installed; editor-local tooling,
  not a repo deliverable).
- **Self-contained local auth (deferred):** local login needs a Cognito pool — there's no
  bundled IdP since the Keycloak→Cognito migration. README now documents "bring your own
  Cognito". A local OIDC container (Keycloak/dex with a `cognito:groups` claim mapper) would
  restore zero-AWS clone-and-run, but partially reverses that migration for local — revisit
  only if the cloner experience needs it.

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
- **Coverage is now a floor gate, not report-only.** `cargo llvm-cov … --fail-under-lines 17`
  (baseline ~18.7% on 2026-06-05). The floor sits under the baseline so it never blocks the
  existing suite but catches a regression; raise deliberately as the suite grows. (2026-06-05)
- **`cargo-deny` added for licenses + bans + sources only** (2026-06-05). Advisories stay with
  `cargo audit` (`.cargo/audit.toml` is the single source of truth) so the two never diverge —
  cargo-deny runs only the non-overlapping checks, resolving the earlier "avoid overlap"
  deferral. Workspace crates are `publish = false` and skipped via `[licenses].private`; three
  permissive transitive licenses (BSL-1.0, CDLA-Permissive-2.0, bzip2-1.0.6) are allow-listed
  with provenance comments in `deny.toml`.
- **`focus_date` is a binary UI model (`today | None`).** The wire type stays
  `Option<NaiveDate>` and the API accepts any date, but the My Day Kanban only ever writes
  `Some(today)` or clears it — there is **no future-date picker on the daily surface**, by
  deliberate user choice ("today or not today", v2.6.0). `due_date` is the editable deadline
  and lives in the task-editor modal. `is_in_my_day`/`list_my_day` still handle carryovers
  (past `focus_date`, not done). A genuine "schedule for a future day" need should land next
  to `due_date`, never as a Kanban quick action — keep the daily surface lean. (2026-04-28)
- **Quick-capture target is a `Task` with `node_id IS NULL`, not a Node.** `/api/inbox/quick`
  (+ the iOS Web Share Target) creates a triage Task surfaced by `/tasks/inbox`
  (`tasks WHERE node_id IS NULL`); wire format `{title?, body?}`, coalesced + truncated to 500
  chars server-side. A 6th `NodeType::Inbox` variant was considered and **rejected** — it
  would have meant a migration plus duplicate sidebar/filter/dashboard wiring for no
  behavioural win, and Notes need a parent node so couldn't be the inbox surface. Future
  capture paths (command palette, Apple Shortcut, third-party clippers) MUST hit
  `/api/inbox/quick` — do not invent a parallel create-node path. (2026-04-27)
