# Ember Trove — Playwright smoke tests

Browser-level smoke tests for the flows host-side `cargo test` structurally
cannot cover: WASM runtime behavior, keyboard listeners, toasts, autosave.
Both v2.21.1 hotfix bugs (zombie window-listener panic; silently-dropped undo
toasts) are pinned here as regression tests.

## How auth works (and why it's safe)

The suite runs against a dedicated Docker stack
(`deploy/docker-compose.e2e.yml` layered over the dev compose) whose api image
is built with the `e2e-bypass` cargo feature and armed with
`E2E_AUTH_BYPASS=1`. Every request is then authenticated as a synthetic
non-admin test user — no Cognito involved, fully offline.

Production cannot grow this code path:

* `deploy/Dockerfile.api` defaults `CARGO_FEATURES` to empty — release images
  are compiled **without** the bypass.
* Even a binary that has the feature compiled in does nothing special unless
  `E2E_AUTH_BYPASS=1` is also set at runtime.

The e2e Postgres uses tmpfs (fresh DB per `up`) under a separate compose
project (`ember-e2e`), so the dev stack's data is never touched.

## Running locally (no Node required)

```bash
./scripts/e2e.sh            # brings the stack up, runs the suite in the
                            # official Playwright Docker image, tears down
KEEP_STACK=1 ./scripts/e2e.sh   # leave the stack running for iteration
```

With Node installed you can iterate faster against a running stack:

```bash
cd e2e && npm ci && npx playwright test            # or --ui for headed mode
```

## CI

The `e2e` job in `.github/workflows/ci.yml` runs the same script with a
native Node + Chromium install. The Playwright HTML report is uploaded as an
artifact on failure.
