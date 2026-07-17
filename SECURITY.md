# Security Policy

Ember Trove is a self-hosted PKM that stores users' personal data, and security
is its first design concern (see [`.claude/POLICY.md`](.claude/POLICY.md) §1).
Reports are taken seriously and appreciated.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report privately via GitHub's vulnerability reporting:

**<https://github.com/jchultarsky/ember-trove/security/advisories/new>**

Include what you can: affected endpoint/component, reproduction steps, impact
assessment, and any suggested fix. You should receive an acknowledgement within
**72 hours**. Please allow a reasonable disclosure window for a fix to ship
before any public write-up; you will be credited in the advisory and the
`CHANGELOG.md` `Security` section unless you prefer otherwise.

## Scope

In scope:

- The `api` crate (Axum backend): authentication/authorization bypasses,
  owner-scoping failures, SQL injection, SSRF (e.g. webhook URL validation),
  secret/PII leakage in logs or error responses.
- The `ui` crate (Leptos/WASM frontend): XSS — especially anything that
  survives the `ammonia` markdown sanitizer — CSRF, token handling.
- Deployment configuration in `deploy/` as shipped.

Out of scope:

- Vulnerabilities in your own hosting environment or modified deployments.
- Denial of service requiring authenticated, owner-level access.
- Reports from automated scanners without a demonstrated impact.

## Supported versions

Ember Trove is a rolling-release, self-hosted application: **only the latest
tagged release** receives security fixes. Dependency advisories are gated in CI
(`cargo audit`, `cargo deny`) on every push.
