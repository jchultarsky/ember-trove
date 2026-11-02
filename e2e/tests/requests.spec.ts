import { test, expect, createNode, type Page } from './fixtures';

/**
 * Requests a page load sends (performance P1 #2). Each one was a round trip
 * on a phone, and over HTTP/1.1 they queued six at a time. These pin the
 * duplicates that were removed, so they don't creep back.
 */

/** API GET paths requested while `load` runs, in order. */
async function apiGets(page: Page, load: () => Promise<void>): Promise<string[]> {
  const seen: string[] = [];
  const on = (r: import('@playwright/test').Request) => {
    const u = new URL(r.url());
    if (u.pathname.startsWith('/api/') && r.method() === 'GET') seen.push(u.pathname);
  };
  page.on('request', on);
  await load();
  await page.waitForLoadState('networkidle');
  page.off('request', on);
  return seen;
}

const count = (seen: string[], path: string) => seen.filter((p) => p === path).length;

test('start-up asks for the version once, and templates only when needed', async ({ page }) => {
  const seen = await apiGets(page, async () => {
    await page.goto('/tasks/my-day');
    await expect(page.locator('main')).toBeVisible();
  });
  // The loading screen's own check (before the app runs) and the app's one.
  // The update banner used to send a third.
  expect(count(seen, '/api/health')).toBe(2);
  // Two create dialogs are always mounted; neither is open.
  expect(count(seen, '/api/templates')).toBe(0);
  expect(count(seen, '/api/auth/me')).toBe(1);
});

test('a create dialog loads the templates when it opens', async ({ page }) => {
  await page.goto('/tasks/my-day');
  await expect(page.locator('main')).toBeVisible();
  await page.waitForLoadState('networkidle');
  const seen = await apiGets(page, async () => {
    // Quick capture's "More fields…" hands its text to the create dialog.
    await page.locator('main').click({ position: { x: 5, y: 5 } });
    await page.keyboard.press('n');
    await page.getByPlaceholder(/What’s on your mind/).fill('e2e requests dialog');
    await page.getByRole('button', { name: /More fields/ }).click();
    await expect(page.getByRole('dialog', { name: 'Create node' })).toBeVisible();
  });
  expect(count(seen, '/api/templates')).toBe(1);
});

test('a node page reads the editor prefs once for both panels', async ({ page, request }) => {
  const node = await createNode(request, `e2e requests ${Date.now()}`, { node_type: 'project' });
  try {
    const seen = await apiGets(page, async () => {
      await page.goto(`/nodes/${node.id}`);
      await expect(page.getByTestId('node-tasks')).toBeVisible({ timeout: 15_000 });
    });
    expect(count(seen, '/api/editor-prefs')).toBe(1);
    // No endpoint twice in one node-page load.
    const dupes = [...new Set(seen)].filter(
      (p) => count(seen, p) > 1 && p !== '/api/health',
    );
    expect(dupes).toEqual([]);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
