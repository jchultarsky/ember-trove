import { test, expect, type Page } from './fixtures';

/**
 * Loading screen version (#120). The splash names its own bundle version,
 * stamped at build time (ui/scripts/stamp-splash-version.sh), and appends the
 * server's version only when the two differ — the stale-cached-shell case.
 *
 * The service worker is blocked so `page.route` sees every request, and the
 * WASM download is held so the splash stays up long enough to read.
 */
test.use({ serviceWorkers: 'block' });

/** Hold the WASM bundle until `release()`, keeping the splash on screen. */
async function holdWasm(page: Page): Promise<() => void> {
  let release!: () => void;
  const held = new Promise<void>((r) => (release = r));
  await page.route(/_bg\.wasm$/, async (route) => {
    await held;
    await route.continue();
  });
  return release;
}

async function serverVersion(page: Page): Promise<string> {
  const resp = await page.request.get('/api/health');
  expect(resp.ok()).toBeTruthy();
  return (await resp.json()).version;
}

test('the served splash carries the stamped release version', async ({ page }) => {
  const version = await serverVersion(page);
  const html = await (await page.request.get('/')).text();
  expect(html).not.toContain('%%APP_VERSION%%');
  expect(html).toContain(`data-version="${version}">v${version}</span>`);
});

test('a stuck load shows the version; matching server adds nothing', async ({ page }) => {
  const version = await serverVersion(page);
  const release = await holdWasm(page);
  const health = page.waitForResponse('**/api/health');
  await page.goto('/tasks/my-day', { waitUntil: 'domcontentloaded' });
  await health;
  await expect(page.locator('#etl-version')).toHaveText(`v${version}`);
  release();
  // The app still mounts and removes the splash.
  await expect(page.locator('main')).toBeVisible();
  await expect(page.locator('#app-loader')).toHaveCount(0);
});

test('a version mismatch with the server names both', async ({ page }) => {
  const version = await serverVersion(page);
  const release = await holdWasm(page);
  await page.route('**/api/health', (route) =>
    route.fulfill({
      json: { status: 'ok', service: 'ember-trove-api', database: 'ok', version: '9.9.9' },
    }),
  );
  await page.goto('/tasks/my-day', { waitUntil: 'domcontentloaded' });
  await expect(page.locator('#etl-version')).toHaveText(`v${version} · server v9.9.9`);
  release();
  await expect(page.locator('main')).toBeVisible();
});

test('a failed health check leaves the stamped version alone', async ({ page }) => {
  const version = await serverVersion(page);
  const release = await holdWasm(page);
  const failed = new Promise<void>((resolve) =>
    page.route('**/api/health', async (route) => {
      await route.fulfill({ status: 503, body: 'unavailable' });
      resolve();
    }),
  );
  await page.goto('/tasks/my-day', { waitUntil: 'domcontentloaded' });
  await failed;
  await expect(page.locator('#etl-version')).toHaveText(`v${version}`);
  release();
  await expect(page.locator('main')).toBeVisible();
});
