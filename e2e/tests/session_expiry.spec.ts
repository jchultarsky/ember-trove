import { test, expect, createNode, type Page } from './fixtures';

/**
 * A tab left open past the access token's lifetime (#190). Downloads were
 * bare navigations that skipped the renew-in-place path, and a reload went
 * straight to the hosted login on the first 401 from `/auth/me`. The e2e
 * stack has no OIDC, so the expiry is a faked 401 and the refresh a faked
 * success (as in save_state.spec.ts).
 */

/** Fake a successful refresh; returns a counter of refresh calls. */
async function fakeRefresh(page: Page) {
  const count = { n: 0 };
  await page.route('**/api/auth/refresh', (route) => {
    count.n += 1;
    return route.fulfill({ status: 200, contentType: 'application/json', body: '{"ok":true}' });
  });
  return count;
}

/** Answer the next `/auth/me` with 401 (an expired access token). */
async function expireOnce(page: Page) {
  let expired = false;
  await page.route('**/api/auth/me', (route) => {
    if (expired) return route.fallback();
    expired = true;
    return route.fulfill({ status: 401, body: '{"error":"expired"}' });
  });
}

test('a reload with an expired access token renews instead of signing in again', async ({ page }) => {
  const refreshes = await fakeRefresh(page);
  await expireOnce(page);
  await page.goto('/');
  await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });
  expect(refreshes.n).toBe(1);
  await expect(page.getByText('Redirecting to login...')).toHaveCount(0);
});

test('the node export renews an expired session before downloading', async ({ page, request }) => {
  const node = await createNode(request, `e2e export renew ${Date.now()}`, { body: 'hello' });
  try {
    await page.goto(`/nodes/${node.id}`);
    await expect(page.getByTestId('node-tasks')).toBeVisible({ timeout: 15_000 });
    const refreshes = await fakeRefresh(page);
    // The token expires while the page sits open.
    await expireOnce(page);
    const download = page.waitForEvent('download');
    await page.locator('a[title="Export as Markdown"]').click();
    expect((await download).suggestedFilename()).toMatch(/\.md$/);
    expect(refreshes.n).toBe(1);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
