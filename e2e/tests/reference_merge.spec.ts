import { test, expect, E2E_SUBJECT, type Page } from './fixtures';

/**
 * Resource merged into Reference (#222, ONTOLOGY §3.1): four node kinds.
 * No surface offers Resource any more, the API refuses it, and the two
 * things a browser may still hold from before 3.0 (an old `?type=resource`
 * link, an unsaved create draft of a Resource) open as a Reference.
 *
 * The draft lives in localStorage, so the spec clears it before AND after
 * each attempt (retry-safety, .claude/rules/e2e.md).
 */

const DRAFT_KEY = `ember_trove_create_draft:${E2E_SUBJECT}`;
const typeSelect = (page: Page) => page.locator('select:has(option[value="reference"])').first();

async function openApp(page: Page) {
  await page.goto('/tasks/my-day');
  // Let the WASM bundle finish loading: a second goto would abort it.
  await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });
}

async function clearDraft(page: Page) {
  await page.evaluate((k) => window.localStorage.removeItem(k), DRAFT_KEY);
}

test.describe('Resource is now Reference', () => {
  test.beforeEach(async ({ page }) => {
    await openApp(page);
    await clearDraft(page);
  });
  test.afterEach(async ({ page }) => {
    await clearDraft(page);
  });

  test('no surface offers Resource', async ({ page }) => {
    // Sidebar: the type filters and their "+" buttons.
    await expect(page.getByTestId('new-reference')).toBeVisible();
    await expect(page.getByTestId('new-resource')).toHaveCount(0);
    await expect(page.getByText('Resources', { exact: true })).toHaveCount(0);

    // Palette: "New Re" finds New Reference and nothing else of that kind.
    await page.keyboard.press('ControlOrMeta+k');
    const input = page.getByPlaceholder('Search nodes, or type a new title…');
    await expect(input).toBeFocused();
    await input.fill('New Re');
    const palette = page.getByRole('dialog');
    await expect(palette.getByRole('button', { name: 'menu_book New Reference', exact: true })).toBeVisible();
    await expect(palette.getByRole('button', { name: 'bookmarks New Resource', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');

    // The create editor's type picker.
    await page.goto('/nodes/new');
    await expect(typeSelect(page)).toBeVisible();
    await expect(typeSelect(page).locator('option')).toHaveText(['Article', 'Project', 'Area', 'Reference']);

    // Search type chips.
    await page.goto('/search');
    await expect(page.getByRole('button', { name: 'Reference', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Resource', exact: true })).toHaveCount(0);

    // Graph legend.
    await page.goto('/graph');
    await expect(page.getByRole('button', { name: 'Fit', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Reference', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Resource', exact: true })).toHaveCount(0);
  });

  test('an old ?type=resource link opens a Reference', async ({ page }) => {
    await page.goto('/nodes/new?type=resource');
    await expect(typeSelect(page)).toHaveValue('reference');
  });

  test('an unsaved Resource draft comes back as a Reference', async ({ page }) => {
    const title = `e2e old resource draft ${Date.now()}`;
    await page.evaluate(
      ([k, t]) =>
        window.localStorage.setItem(
          k,
          JSON.stringify({ title: t, body: 'kept', node_type: 'resource', status: 'draft', saved_at: Date.now() }),
        ),
      [DRAFT_KEY, title],
    );
    await page.goto('/nodes/new');
    await expect(page.getByPlaceholder('Node title…')).toHaveValue(title);
    await expect(typeSelect(page)).toHaveValue('reference');
  });

  test('the API refuses resource', async ({ request }) => {
    const create = await request.post('/api/nodes', {
      data: { title: `e2e resource ${Date.now()}`, node_type: 'resource', body: '' },
    });
    // serde rejects the unknown variant before the handler runs.
    expect(create.status()).toBe(422);
    const lookup = await request.get('/api/nodes/lookup?q=e2e&node_type=resource');
    expect(lookup.status()).toBe(400);
  });
});
