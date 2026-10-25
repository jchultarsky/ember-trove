import { test, expect, createNode, createNote } from './fixtures';

/**
 * Wiki links resolve on demand (#134): a node page asks the server for just
 * the titles its body links (GET /api/nodes/resolve-titles) instead of
 * downloading every node title, and matching is case-insensitive like the
 * server's edge sync. The editor's [[ suggestions and the Notes feed's node
 * filter no longer need the full title list either.
 */

test('a node page resolves its links without the full title list', async ({ page, request }) => {
  const stamp = Date.now();
  const target = await createNode(request, `Wiki Target ${stamp}`);
  const source = await createNode(request, `e2e wiki source ${stamp}`, {
    // Different case on purpose: resolution is case-insensitive.
    body: `See [[wiki target ${stamp}]] and [[Missing ${stamp}|a missing one]].`,
  });
  try {
    const fullListFetches: string[] = [];
    page.on('request', (r) => {
      if (new URL(r.url()).pathname === '/api/nodes/titles') fullListFetches.push(r.url());
    });
    await page.goto(`/nodes/${source.id}`);
    const link = page.locator('a.wikilink', { hasText: `wiki target ${stamp}` });
    await expect(link).toBeVisible();
    await expect(page.locator('.wikilink-unresolved', { hasText: 'a missing one' })).toBeVisible();
    expect(fullListFetches).toEqual([]);
    await link.click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${target.id}$`));
  } finally {
    await request.delete(`/api/nodes/${source.id}`);
    await request.delete(`/api/nodes/${target.id}`);
  }
});

test('the editor suggests link titles from the lookup', async ({ page, request }) => {
  const stamp = Date.now();
  const target = await createNode(request, `Suggest Target ${stamp}`);
  const source = await createNode(request, `e2e suggest source ${stamp}`);
  try {
    await page.goto(`/nodes/${source.id}/edit`);
    const editor = page.getByPlaceholder(/Write in Markdown/);
    await expect(editor).toBeVisible();
    await editor.click();
    await editor.pressSequentially(`See [[Suggest Target ${stamp}`.slice(0, -4), { delay: 15 });
    const dropdown = page.getByText(/Link to node —/).locator('..');
    await expect(dropdown.getByRole('button', { name: new RegExp(`Suggest Target ${stamp}`) })).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${source.id}`);
    await request.delete(`/api/nodes/${target.id}`);
  }
});

test('the Notes feed filters by a node picked with the shared picker', async ({ page, request }) => {
  const stamp = Date.now();
  const a = await createNode(request, `e2e notes filter a ${stamp}`);
  const b = await createNode(request, `e2e notes filter b ${stamp}`);
  await createNote(request, a.id, `note on a ${stamp}`);
  await createNote(request, b.id, `note on b ${stamp}`);
  try {
    await page.goto('/notes');
    await expect(page.getByText(`note on a ${stamp}`)).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText(`note on b ${stamp}`)).toBeVisible();
    await page.locator('select[title="Filter by node"]').selectOption('node');
    const picker = page
      .getByTestId('notes-node-filter')
      .getByRole('combobox', { name: 'Filter by node…' });
    await expect(picker).toBeFocused();
    await picker.fill(`e2e notes filter a ${stamp}`);
    await picker.press('Enter');
    await expect(page.getByText(`note on b ${stamp}`)).toHaveCount(0);
    await expect(page.getByText(`note on a ${stamp}`)).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${a.id}`);
    await request.delete(`/api/nodes/${b.id}`);
  }
});
