import { test as base, expect, type APIRequestContext } from '@playwright/test';

/**
 * Shared e2e helpers. Every spec imports `test` / `expect` from here, not
 * from '@playwright/test', so the page-error guard below covers the whole
 * suite (it used to be a hand-copied `collectPageErrors` in four specs, and
 * missing from the keyboard-heavy palette and triage specs).
 */

export { expect };
export type { APIRequestContext, Locator, Page } from '@playwright/test';

export const test = base.extend<{
  pageErrors: string[];
  cspViolations: string[];
  otherRequest: APIRequestContext;
}>({
  /**
   * API calls as the SECOND e2e identity (#135), for sharing and permission
   * tests. The stack's auth bypass maps `x-e2e-subject: other` to a fixed
   * test subject (`e2e-other-user`); the default identity is
   * `e2e-test-user`. Only e2e-bypass builds honour the header.
   */
  otherRequest: async ({ playwright, baseURL }, use) => {
    const ctx = await playwright.request.newContext({
      baseURL,
      extraHTTPHeaders: { 'x-e2e-subject': 'other' },
    });
    await use(ctx);
    await ctx.dispose();
  },
  /**
   * Fails the test on any uncaught exception or WASM panic. A panic poisons
   * event dispatch silently — later clicks just do nothing — so a spec can
   * otherwise pass on a broken page (the v2.21.1 lesson, .claude/ERRORS.md).
   * Navigating again before the WASM bundle has loaded aborts its download
   * and surfaces here as "TypeError: Failed to fetch": wait for the app to
   * render (`main` visible) before the next `goto` (.claude/rules/e2e.md).
   */
  pageErrors: [
    async ({ page }, use) => {
      const errors: string[] = [];
      page.on('pageerror', (err) => errors.push(err.stack || String(err)));
      await use(errors);
      expect(errors, 'uncaught page errors').toEqual([]);
    },
    { auto: true },
  ],
  /**
   * Fails the test on any Content-Security-Policy violation. A blocked
   * `eval`, script, or fetch does not always throw — the old js_sys::eval
   * date/clipboard helpers just failed silently — so `pageErrors` alone
   * can't prove the app runs under the real policy (no 'unsafe-eval').
   * Listens for the spec-defined `securitypolicyviolation` event; the init
   * script itself is injected by Playwright and is not subject to the CSP.
   */
  cspViolations: [
    async ({ page }, use) => {
      const violations: string[] = [];
      await page.exposeBinding('__e2eCspViolation', (_src, v: string) => {
        violations.push(v);
      });
      await page.addInitScript(() => {
        document.addEventListener('securitypolicyviolation', (e) => {
          const report = (window as unknown as { __e2eCspViolation: (v: string) => void })
            .__e2eCspViolation;
          report(`${e.effectiveDirective} blocked ${e.blockedURI} at ${e.sourceFile}:${e.lineNumber}`);
        });
      });
      await use(violations);
      expect(violations, 'CSP violations').toEqual([]);
    },
    { auto: true },
  ],
});

/** Local calendar date `offsetDays` from today, as `YYYY-MM-DD`. */
export function localISO(offsetDays: number): string {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`;
}

/** Create a draft node via the API (the e2e bypass authenticates `request`). */
export async function createNode(
  request: APIRequestContext,
  title: string,
  opts: { node_type?: string; body?: string } = {},
) {
  const resp = await request.post('/api/nodes', {
    data: {
      title,
      node_type: opts.node_type ?? 'article',
      body: opts.body ?? '',
      metadata: {},
      status: 'draft',
    },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

/** Add a note to a node via the API. */
export async function createNote(request: APIRequestContext, nodeId: string, body: string) {
  const resp = await request.post(`/api/nodes/${nodeId}/notes`, { data: { body, color: 'default' } });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

/** The default e2e identity's subject (what the second identity shares with). */
export const E2E_SUBJECT = 'e2e-test-user';

/** Share `nodeId` (owned by the caller of `request`) with `subject`. */
export async function shareNode(
  request: APIRequestContext,
  nodeId: string,
  subject: string,
  role: 'viewer' | 'editor' | 'owner' = 'viewer',
) {
  const resp = await request.post(`/api/nodes/${nodeId}/permissions`, {
    data: { subject_id: subject, role },
  });
  expect(resp.ok()).toBeTruthy();
}
