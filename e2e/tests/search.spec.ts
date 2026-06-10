import { test, expect } from '@playwright/test';

/**
 * Saved search presets: save the current filters under a name, reload-persist,
 * load on click, delete. (The UI predates the e2e suite — this pins it.)
 */

test('save, reload, load, and delete a search preset', async ({ page }) => {
  const name = `e2e preset ${Date.now()}`;

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

  // Survives a reload.
  await page.reload();
  await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });
  await page.getByTitle('Saved search presets').click();
  const row = page.getByRole('button', { name: new RegExp(name) });
  await expect(row).toBeVisible();

  // Loading closes the menu and applies the query to the search box.
  await row.click();
  await expect(page.getByRole('button', { name: 'Save current search' })).not.toBeVisible();
  await expect(searchBox).toHaveValue('preset-query');

  // Delete → empty state.
  await page.getByTitle('Saved search presets').click();
  await page.getByTitle('Delete preset').click();
  await expect(page.getByText('No presets saved')).toBeVisible();
});
