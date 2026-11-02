import { test, expect, type APIRequestContext } from './fixtures';

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

test('header search keeps the query on /search and clears it on leaving', async ({
  page,
  request,
}) => {
  // #98 item 4: a stale term left in the header read as if it filtered the
  // node being viewed. Unique per attempt, so retries never collide.
  const token = `zq${Date.now()}`;
  const resp = await request.post('/api/nodes', {
    data: { title: `e2e header ${token}`, node_type: 'article', body: '', metadata: {}, status: 'draft' },
  });
  expect(resp.ok()).toBeTruthy();
  const node = await resp.json();
  try {
    await page.goto('/tasks/captures');
    const searchBox = page.getByPlaceholder('Search… (Enter)');
    await searchBox.fill(token);
    await searchBox.press('Enter');
    await expect(page).toHaveURL(/\/search$/);
    await expect(searchBox).toHaveValue(token); // kept for the results page

    // Open the result → the header no longer carries the term.
    // Result titles go through the search headline (which may trim words),
    // so locate the result by the unique token, inside the results pane.
    await page.locator('main').getByText(token).first().click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}`));
    await expect(searchBox).toHaveValue('');
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

// ── Result cards: full titles, metadata line, match badges (#107, #108) ─────

test('result headings are full titles, with type/updated meta and no slug', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const titleWord = `sam${stamp}`;
  const bodyWord = `tile${stamp}`;
  const mk = async (title: string, body: string) => {
    const r = await request.post('/api/nodes', {
      data: { title, node_type: 'project', body, metadata: {}, status: 'draft' },
    });
    expect(r.ok()).toBeTruthy();
    return r.json();
  };
  const gift = await mk(`Buy a gift for ${titleWord}`, 'wrapping paper and a card');
  const bath = await mk('Master Bath Remodel', `pick the ${bodyWord} and the grout`);
  try {
    const searchBox = page.getByPlaceholder('Search… (Enter)');
    const main = page.locator('main');

    // Title match: the whole title, the matched word marked, "matched in title".
    await page.goto('/search');
    await searchBox.fill(titleWord);
    await searchBox.press('Enter');
    const giftCard = main.getByRole('button').filter({ hasText: titleWord });
    await expect(giftCard.getByRole('heading')).toHaveText(`Buy a gift for ${titleWord}`);
    await expect(giftCard.getByRole('heading').locator('mark')).toHaveText(titleWord);
    const meta = giftCard.getByTestId('result-meta');
    await expect(meta).toContainText('Project');
    await expect(meta).toContainText('Updated');
    await expect(meta).toContainText('matched in title');
    // The slug (an opaque id) is gone from the card.
    await expect(giftCard).not.toContainText(gift.slug);

    // Body-only match: still the full title (was "Master"), "matched in text".
    await searchBox.fill(bodyWord);
    await searchBox.press('Enter');
    const bathCard = main.getByRole('button').filter({ hasText: 'Master Bath Remodel' });
    await expect(bathCard.getByRole('heading')).toHaveText('Master Bath Remodel');
    await expect(bathCard.getByTestId('result-meta')).toContainText('matched in text');
    await expect(bathCard.getByTestId('result-snippet').locator('mark')).toHaveText(bodyWord);
  } finally {
    await request.delete(`/api/nodes/${gift.id}`);
    await request.delete(`/api/nodes/${bath.id}`);
  }
});

test('an empty query is framed as "Recently updated"', async ({ page }) => {
  await page.goto('/search');
  await expect(page.locator('main').getByText(/^Recently updated · \d+ nodes?/)).toBeVisible();
});
