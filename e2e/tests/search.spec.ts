import { test, expect, APIRequestContext } from '@playwright/test';

/**
 * Saved search presets: save the current filters under a name, reload-persist,
 * load on click, delete. (The UI predates the e2e suite — this pins it.)
 */

/**
 * Presets live in the shared DB, so a mid-test failure leaves its preset
 * behind and poisons the Playwright retry: two rows means the unscoped
 * "Delete preset" click hits a strict-mode violation and the "No presets
 * saved" empty state never shows. Wipe before AND after every attempt.
 */
async function wipePresets(request: APIRequestContext) {
  const presets = await (await request.get('/api/search-presets')).json();
  for (const p of presets) await request.delete(`/api/search-presets/${p.id}`);
}

test('save, reload, load, and delete a search preset', async ({ page, request }) => {
  await wipePresets(request);
  const name = `e2e preset ${Date.now()}`;
  try {
    await page.goto('/search');
    await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });

    // Set a query via the sidebar search bar (it writes the shared context).
    const searchBox = page.getByPlaceholder('Search… (Enter)');
    await searchBox.fill('preset-query');
    await searchBox.press('Enter');

    // Save the current search as a preset.
    await page.getByTitle('Saved search presets').click();
    await page.getByRole('button', { name: 'Save current search' }).click();
    await page.getByPlaceholder('Preset name…').fill(name);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.getByRole('button', { name: new RegExp(name) })).toBeVisible();

    // Survives a reload. The preset list is fetched after the menu opens —
    // give it the same cold-runner allowance as the app render.
    await page.reload();
    await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });
    await page.getByTitle('Saved search presets').click();
    const row = page.getByRole('button', { name: new RegExp(name) });
    await expect(row).toBeVisible({ timeout: 15_000 });

    // Loading closes the menu and applies the query to the search box.
    await row.click();
    await expect(page.getByRole('button', { name: 'Save current search' })).not.toBeVisible();
    await expect(searchBox).toHaveValue('preset-query');

    // Delete → empty state.
    await page.getByTitle('Saved search presets').click();
    await page.getByTitle('Delete preset').click();
    await expect(page.getByText('No presets saved')).toBeVisible();
  } finally {
    await wipePresets(request);
  }
});
