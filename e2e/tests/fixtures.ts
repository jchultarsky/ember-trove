import { test as base, expect, type APIRequestContext } from '@playwright/test';

/**
 * Shared e2e helpers. Every spec imports `test` / `expect` from here, not
 * from '@playwright/test', so the page-error guard below covers the whole
 * suite (it used to be a hand-copied `collectPageErrors` in four specs, and
 * missing from the keyboard-heavy palette and triage specs).
 */

export { expect };
export type { APIRequestContext, Page } from '@playwright/test';

export const test = base.extend<{ pageErrors: string[] }>({
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
