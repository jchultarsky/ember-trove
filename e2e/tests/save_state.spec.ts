import { test, expect, createNode, type APIResponse, type Page, type Route } from './fixtures';

/**
 * #161: the node editor's save state is honest (saving / still saving /
 * failed + Retry / saved) and unsaved edits survive in a local draft.
 * Saves are slowed or failed with `page.route` on the node's PUT only;
 * the GET that loads the editor goes through untouched.
 */

async function routeSaves(page: Page, id: string, handle: 'fail' | 'slow') {
  await page.route(`**/api/nodes/${id}`, async (route) => {
    if (route.request().method() !== 'PUT') return route.continue();
    if (handle === 'fail') return route.abort('failed');
    await new Promise((r) => setTimeout(r, 7_000)); // past STALL_AFTER_MS
    return route.continue();
  });
}

async function openEditor(page: Page, id: string) {
  await page.goto(`/nodes/${id}/edit`);
  const editor = page.getByPlaceholder(/Write in Markdown/);
  await expect(editor).toBeVisible();
  return editor;
}

test('a failed save offers Retry, and Retry saves', async ({ page, request }) => {
  const node = await createNode(request, `e2e save retry ${Date.now()}`, { body: 'before' });
  try {
    await routeSaves(page, node.id, 'fail');
    const editor = await openEditor(page, node.id);
    await editor.fill('after');
    const retry = page.getByTestId('save-retry');
    await expect(retry).toBeVisible({ timeout: 10_000 });

    await page.unroute(`**/api/nodes/${node.id}`);
    await retry.click();
    await expect(page.getByText(/^Saved/)).toBeVisible({ timeout: 10_000 });
    await expect(retry).toHaveCount(0);
    const saved = await (await request.get(`/api/nodes/${node.id}`)).json();
    expect(saved.body).toBe('after');
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('a slow save says "Still saving…" and then lands', async ({ page, request }) => {
  const node = await createNode(request, `e2e save slow ${Date.now()}`, { body: 'before' });
  try {
    await routeSaves(page, node.id, 'slow');
    const editor = await openEditor(page, node.id);
    await editor.fill('slow but sure');
    await expect(page.getByText('Still saving…')).toBeVisible({ timeout: 10_000 });
    await expect(page.getByText(/^Saved/)).toBeVisible({ timeout: 15_000 });
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('edits that never reached the server are offered back', async ({ page, request }) => {
  const node = await createNode(request, `e2e save draft ${Date.now()}`, { body: 'before' });
  try {
    await routeSaves(page, node.id, 'fail');
    const editor = await openEditor(page, node.id);
    await editor.fill('kept on this device');
    await expect(page.getByTestId('save-retry')).toBeVisible({ timeout: 10_000 });

    // Same browser context (same localStorage), no routes: as if the user
    // came back after a reload. A second page avoids the beforeunload prompt.
    const again = await page.context().newPage();
    const editor2 = await openEditor(again, node.id);
    await expect(editor2).toHaveValue('before');
    const offer = again.getByTestId('draft-recovery');
    await expect(offer).toBeVisible();
    await offer.getByRole('button', { name: 'Restore' }).click();
    await expect(editor2).toHaveValue('kept on this device');
    await expect(again.getByText(/^Saved/)).toBeVisible({ timeout: 10_000 });
    const saved = await (await request.get(`/api/nodes/${node.id}`)).json();
    expect(saved.body).toBe('kept on this device');

    // Once saved, the draft is gone: a fresh visit offers nothing.
    const third = await page.context().newPage();
    const editor3 = await openEditor(third, node.id);
    await expect(editor3).toHaveValue('kept on this device'); // loaded
    await expect(third.getByTestId('draft-recovery')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

// ── #161 follow-up ────────────────────────────────────────────────────────────

test.describe('save follow-up (#161)', () => {
  test('a save stuck in the 401 refresh path is given up and Retry works', async ({
    page,
    request,
  }) => {
    test.setTimeout(60_000);
    const node = await createNode(request, `e2e save parked ${Date.now()}`, { body: 'before' });
    // The PUT gets a 401, so the API client calls /api/auth/refresh, which
    // never answers: the save's future never settles (the path that left
    // the editor on "Still saving…" forever). The refresh route stays held
    // for the rest of the test (unrouting would release it to the network).
    await page.route('**/api/auth/refresh', () => {});
    await page.route(`**/api/nodes/${node.id}`, (route) =>
      route.request().method() === 'PUT'
        ? route.fulfill({ status: 401, body: '{"error":"expired"}' })
        : route.fallback(),
    );
    try {
      const editor = await openEditor(page, node.id);
      await editor.fill('parked');
      await expect(page.getByText('Still saving…')).toBeVisible({ timeout: 10_000 });
      const retry = page.getByTestId('save-retry');
      await expect(retry).toBeVisible({ timeout: 15_000 });
      await expect(page.getByText(/took too long and was stopped/)).toBeVisible();

      await page.unroute(`**/api/nodes/${node.id}`);
      await retry.click();
      await expect(page.getByText(/^Saved/)).toBeVisible({ timeout: 10_000 });
      expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe('parked');
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('a save that lands after it was given up never overwrites the retry', async ({
    page,
    request,
  }) => {
    test.setTimeout(60_000);
    const node = await createNode(request, `e2e save late ${Date.now()}`, { body: 'before' });
    // The first PUT reaches the API (and commits), but its answer is
    // withheld past the give-up: the "landed late" case. Later PUTs pass.
    const held: { route: Route; resp: APIResponse }[] = [];
    await page.route(`**/api/nodes/${node.id}`, async (route) => {
      if (route.request().method() !== 'PUT' || held.length > 0) return route.fallback();
      held.push({ route, resp: await route.fetch() });
    });
    try {
      const editor = await openEditor(page, node.id);
      await editor.fill('first');
      const retry = page.getByTestId('save-retry');
      await expect(retry).toBeVisible({ timeout: 25_000 });
      expect(held).toHaveLength(1);

      // Newer text: autosave sends it on the version the editor loaded, the
      // API refuses it (412) because the first save landed, the editor sees
      // its own text there and re-sends on top.
      await editor.fill('newer');
      await expect(page.getByText(/^Saved/)).toBeVisible({ timeout: 10_000 });
      const saved = await (await request.get(`/api/nodes/${node.id}`)).json();
      expect(saved.body).toBe('newer');

      // The stale save, replayed now, is refused by its version (412).
      const replay = await request.put(`/api/nodes/${node.id}`, {
        data: { body: 'first', expected_updated_at: node.updated_at },
      });
      expect(replay.status()).toBe(412);
      expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe('newer');
    } finally {
      await Promise.all(held.map((h) => h.route.abort().catch(() => {})));
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('a change made elsewhere is not overwritten unasked', async ({ page, request }) => {
    const node = await createNode(request, `e2e save foreign ${Date.now()}`, { body: 'before' });
    try {
      const editor = await openEditor(page, node.id);
      await expect(editor).toHaveValue('before');
      // Another device saves while this editor is open.
      const other = await request.put(`/api/nodes/${node.id}`, { data: { body: 'elsewhere' } });
      expect(other.ok()).toBeTruthy();

      await editor.fill('mine');
      await expect(page.getByText(/changed somewhere else/)).toBeVisible({ timeout: 10_000 });
      const retry = page.getByTestId('save-retry');
      await expect(retry).toBeVisible();
      expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe('elsewhere');

      // Only the explicit Retry overwrites it.
      await retry.click();
      await expect(page.getByText(/^Saved/)).toBeVisible({ timeout: 10_000 });
      expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe('mine');
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('a new-node draft stays with its own account', async ({ page }) => {
    const title = `e2e private draft ${Date.now()}`;
    const titleInput = () => page.getByPlaceholder('Node title…');
    const openNew = async () => {
      await page.goto('/nodes/new');
      await expect(page.locator('main')).toBeVisible();
      await expect(titleInput()).toBeVisible();
    };

    // A legacy unscoped draft (pre-#161) is deleted unread.
    await page.goto('/');
    await expect(page.locator('main')).toBeVisible();
    await page.evaluate(() =>
      localStorage.setItem(
        'ember_trove_create_draft',
        JSON.stringify({ title: 'legacy', body: 'x', node_type: 'article', status: 'draft' }),
      ),
    );
    await openNew();
    await expect(titleInput()).toHaveValue('');
    expect(await page.evaluate(() => localStorage.getItem('ember_trove_create_draft'))).toBeNull();

    // User A starts a node; the draft is kept locally.
    await titleInput().fill(title);
    await expect(page.getByText('Draft kept locally')).toBeVisible({ timeout: 10_000 });

    // User B in the same browser (same localStorage) gets a blank editor.
    await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
    await openNew();
    await expect(titleInput()).toHaveValue('');

    // A gets the draft back.
    await page.setExtraHTTPHeaders({});
    await openNew();
    await expect(titleInput()).toHaveValue(title);
  });
});
