# E2E / Playwright Rules (auto-relevant for `e2e/`)

Suite architecture and how to run it: `e2e/README.md`. Orchestrator:
`scripts/e2e.sh` (Playwright in Docker — no local Node needed). Iterate with
`KEEP_STACK=1`. **Verify specs against the local stack before pushing** — the
CI job is too slow a feedback loop for selector work.

## Selector gotchas (all hit on day one, 2026-06-10)

- **CSS-uppercased text is not DOM text.** The Kanban zone header *displays*
  "TODAY" but the DOM text is "Today" (Tailwind `uppercase`).
  `getByText('TODAY')` finds nothing. Prefer role-based selectors:
  `getByRole('heading', { name: 'My Day' })`.
- **Material icon glyph names pollute accessible text.** An element like
  `<span><span class="material-symbols-outlined">event</span>"Due Fri…"</span>`
  has text "event Due Fri…", so anchored regexes (`/^Due /`) fail. Use
  unanchored matches: `getByText(/Due (Mon|Tue|Wed|Thu|Fri|Sat|Sun)/)`.
- **Placeholders use the Unicode ellipsis** (`…`, U+2026), not `...` — this
  codebase uses `\u{2026}` throughout. `getByPlaceholder('Task title...')`
  fails; `getByPlaceholder('Task title…')` matches.
- **Hidden duplicates cause strict-mode violations.** Triage hides the inbox
  list with CSS (`hidden`) — the task title then exists twice in the DOM.
  Scope assertions to a container (`getByTestId('triage-card')`); give new
  overlay-style surfaces a `data-testid`.
- **Gate keyboard shortcuts on app render.** A keystroke sent before the
  WASM bundle initializes (cold CI runners!) is silently lost — wait for
  `page.locator('main')` to be visible after `goto` before pressing keys
  (`gotoApp` helper in palette.spec.ts). Clicking a real element first
  (auto-waiting) achieves the same.
- **Visible ≠ focused** (2026-10-17, palette spec): `toBeVisible()` checks
  layout, not focus — before pressing a key whose handler needs focus, assert
  `await expect(input).toBeFocused()`. Better still, make the UI not need it.
- **Element refs go stale across re-renders** — a list refetch between a
  `find` and a click invalidates refs. Re-locate just before acting, or drive
  the assertion through `expect(locator)` auto-waiting.
- **Positional clicks hang on SVG canvas children** (2026-07-17, graph spec):
  `locator.click()`/`.dblclick()` on elements inside the graph `<svg>` time
  out in Playwright's actionability phase (scroll-into-view/stability) even
  though `toBeVisible()` passes. Use `locator.dispatchEvent('click' |
  'dblclick')` — it still runs the app's real handlers. Address graph nodes
  via `g[data-node-id="<uuid>"]`.
- **Graph node titles collide with toolbar button lookups** (2026-07-18):
  every graph node `<g>` is `role="button"` with the node *title* as its
  accessible name (keyboard phase 3), and `getByRole('button', { name })`
  substring-matches by default — a fixture titled "e2e fit far-a" makes
  `getByRole('button', { name: 'Fit' })` a strict-mode violation. Use
  `exact: true` for toolbar buttons on the graph page (`gotoGraph` in
  graph.spec.ts), and don't give fixtures titles that echo toolbar labels.

- **Fixture titles that contain badge words** (2026-10-23, my_day spec): a
  title like "e2e complete carried …" makes `row.getByText(/carried/)` match
  the title as well as the carry-over badge (strict-mode violation). Locate
  badges by `data-testid` (`carryover-prompt`), not by their text.

- **Fill date inputs, never type into them** (2026-10-24): set a native
  `<input type="date">` with `locator.fill('YYYY-MM-DD')`. Typing into its
  Month/Day/Year segments is fragile, and a stray digit silently changes the
  date (the #98 item 1 bug). Task due-date fields are
  `getByTestId('due-date-field')`, with `due-hint` and `due-warning` inside;
  the warning span is always rendered (an aria-live region), so assert
  `toBeEmpty()` / `toHaveText()`, not visibility.

- **Sidebar "+" buttons share names with palette commands** (2026-10-24,
  #111): each type row has a "+" named "New Project" / "New Area" /…, and so do
  the palette commands. Scope palette lookups to the dialog —
  `page.getByRole('dialog').getByRole('button', { name })` — and address the
  sidebar buttons by `data-testid="new-<type>"`.

## Writing new specs

- One spec file per surface; keep the suite **sequential** (`workers: 1`) —
  tests share one backend DB.
- Unique titles via `Date.now()`; clean up created data (API `request`
  fixture is authenticated by the bypass — `finally { request.delete(...) }`).
- **Specs must be retry-safe, not just self-cleaning** (2026-07-18): the
  shared DB persists across Playwright retries, so a mid-test failure leaves
  its fixtures behind and poisons attempt 2 (duplicate rows → strict-mode
  violations; empty states never appear). A `finally` alone doesn't cover a
  crash before it runs on the *first* attempt's leftovers — wipe the spec's
  data via the API **before AND after** each attempt (`wipePresets` in
  search.spec.ts; found via develop run 29662335087, green on PR CI, red only
  on the retry).
- **Import `test`/`expect` from `./fixtures`, never `@playwright/test`**
  (2026-10-24). Its auto fixture fails any test on a `pageerror` — a WASM
  panic poisons event dispatch silently otherwise (the v2.21.1 lesson; see
  `.claude/ERRORS.md`). Shared helpers (`createNode`, `localISO`) live there
  too; add new cross-spec helpers there instead of copying them.
- **The fixtures also fail any test on a CSP violation** (2026-10-24,
  `cspViolations`; the stack's CSP has no `'unsafe-eval'`). To probe the
  guard, use a string `setTimeout` inside `page.evaluate`. A bare `eval()`
  there, or in a `<script>` it injects, runs with DevTools privileges and
  is NOT blocked, so it proves nothing.
- **Don't `goto` again before the app has rendered** (2026-10-24): it aborts
  the WASM bundle download, which the guard reports as `TypeError: Failed to
  fetch` (empty stack; the trace shows the `_bg.wasm` request `ERR_ABORTED`).
  After a setup `goto`, `await expect(page.locator('main')).toBeVisible()`.
- Single-key shortcuts need neutral focus first (`page.locator('main').click()`)
  — the global handler ignores keys from inputs/buttons.
- Use `ControlOrMeta+Enter` for submit chords (portable across macOS/Linux).

## Stack invariants

- The api image must be built with `--features e2e-bypass` AND armed with
  `E2E_AUTH_BYPASS=1`; `scripts/e2e.sh` refuses to run if `/api/auth/me`
  doesn't answer without a session. Release images never carry the feature.
- The same `e2e-bypass` + `E2E_AUTH_BYPASS=1` gate relaxes the API write rate
  limiter (`rate_limit_budget` in `api/src/routes/mod.rs`). Without it the suite
  (one client IP, API-created fixtures) trips HTTP 429 on fast runners —
  symptom: random fixture `resp.ok()` failures / missing toasts (2026-10-16).
- Compose overlay caveats (already encoded in `deploy/docker-compose.e2e.yml`):
  `${VAR:?}` interpolation happens before override merging (the script exports
  COOKIE_KEY), and clearing an inherited list needs `!reset`.
- Bump the `@playwright/test` version and the Docker image tag in
  `scripts/e2e.sh` **together**.
