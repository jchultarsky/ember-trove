import { test, expect, createNode, E2E_SUBJECT, type Page, type Route } from './fixtures';

/**
 * Node-editor save states and local edit drafts (#161).
 *
 * A save that stalls must escalate visibly ("Saving…" → "Still saving…" →
 * "Save failed" + Retry), and edits that never reached the server must
 * survive a reload as a draft that is offered back, never applied silently.
 * The save request is held or failed with `page.route`; the watchdog's real
 * thresholds (5 s / 15 s, `common::save_status`) are used, hence the longer
 * test timeout.
 */

const DRAFT_PREFIX = 'ember_trove_edit_draft:';
/** The editor saves with `PUT /api/nodes/:id`. */
const isSave = (route: Route) => route.request().method() === 'PUT';

async function openEditor(page: Page, id: string, body: string) {
  await page.goto(`/nodes/${id}/edit`);
  const editor = page.getByPlaceholder(/Write in Markdown/);
  await expect(editor).toHaveValue(body);
  return editor;
}

async function editDrafts(page: Page): Promise<string[]> {
  return page.evaluate(
    (prefix) => Object.keys(localStorage).filter((k) => k.startsWith(prefix)),
    DRAFT_PREFIX,
  );
}

/** Hold every save of `id` unanswered; the held routes are returned. */
async function holdSaves(page: Page, id: string): Promise<Route[]> {
  const held: Route[] = [];
  await page.route(`**/api/nodes/${id}`, (route) => {
    if (!isSave(route)) return route.fallback();
    held.push(route);
  });
  return held;
}

/** Type an edit and wait for the watchdog to give up on its save. */
async function stallUntilFailed(page: Page, id: string, start: string, typed: string) {
  const editor = await openEditor(page, id, start);
  await editor.click();
  await editor.press('End');
  await editor.pressSequentially(typed);
  const state = page.getByTestId('save-state');
  await expect(state).toHaveText('Saving…', { timeout: 5_000 });
  await expect(state).toHaveText('Still saving…', { timeout: 8_000 });
  await expect(page.getByTestId('save-retry')).toHaveCount(0);
  await expect(state).toHaveText('Save failed', { timeout: 15_000 });
  await expect(page.getByTestId('save-retry')).toBeVisible();
  return state;
}

test.describe('editor save states (#161)', () => {
  test('a stalled save escalates to failed, and Retry saves it', async ({ page, request }) => {
    test.setTimeout(60_000);
    const node = await createNode(request, `e2e stall ${Date.now()}`, { body: 'start' });
    const held = await holdSaves(page, node.id);
    try {
      const state = await stallUntilFailed(page, node.id, 'start', ' stalled');
      // The edit is kept locally while it is unconfirmed.
      expect(await editDrafts(page)).toHaveLength(1);

      // The stuck request finally dies: a late error changes nothing.
      // Settle held routes BEFORE unroute, which would otherwise release
      // them to the network itself.
      await Promise.all(held.splice(0).map((r) => r.abort('failed')));
      await page.unroute(`**/api/nodes/${node.id}`);
      await expect(state).toHaveText('Save failed');

      // Retry sends the edit for real.
      await page.getByTestId('save-retry').click();
      await expect(state).toHaveText(/^Saved · \d{1,2}:\d{2}/, { timeout: 10_000 });
      await expect(page.getByTestId('save-retry')).toHaveCount(0);

      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('start stalled');
      // Confirmed by the server, so the local draft is gone.
      expect(await editDrafts(page)).toEqual([]);
    } finally {
      await Promise.all(held.map((r) => r.abort().catch(() => {})));
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('a save given up on that lands late clears the failure', async ({ page, request }) => {
    test.setTimeout(60_000);
    const node = await createNode(request, `e2e late ${Date.now()}`, { body: 'start' });
    const held = await holdSaves(page, node.id);
    try {
      const state = await stallUntilFailed(page, node.id, 'start', ' late');
      // The held request reaches the API after all.
      await Promise.all(held.splice(0).map((r) => r.continue()));
      await page.unroute(`**/api/nodes/${node.id}`);
      await expect(state).toHaveText(/^Saved · /, { timeout: 10_000 });
      await expect(page.getByTestId('save-retry')).toHaveCount(0);
      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('start late');
      expect(await editDrafts(page)).toEqual([]);
    } finally {
      await Promise.all(held.map((r) => r.abort().catch(() => {})));
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('edits that never reached the server are offered back after a reload', async ({
    page,
    request,
  }) => {
    const node = await createNode(request, `e2e draft restore ${Date.now()}`, { body: 'base' });
    try {
      await page.route(`**/api/nodes/${node.id}`, (route) =>
        isSave(route) ? route.abort('failed') : route.fallback(),
      );
      const editor = await openEditor(page, node.id, 'base');
      await editor.click();
      await editor.press('End');
      await editor.pressSequentially(' kept');
      await expect(page.getByTestId('save-state')).toHaveText('Save failed', { timeout: 10_000 });
      await expect(page.getByTestId('save-retry')).toBeVisible();

      // Reload, as the 401 refresh path or the update banner would. The
      // editor's beforeunload guard asks first; accept it.
      await page.unroute(`**/api/nodes/${node.id}`);
      page.on('dialog', (d) => d.accept());
      await page.reload();
      await expect(page.locator('main')).toBeVisible();
      await expect(page.getByPlaceholder(/Write in Markdown/)).toHaveValue('base');

      // Offered, not applied: the server copy is still what's shown.
      const offer = page.getByTestId('draft-restore');
      await expect(offer).toContainText('Unsaved edits from');
      await expect(offer).not.toContainText('changed since');
      await offer.getByRole('button', { name: 'Restore' }).click();
      await expect(offer).toHaveCount(0);
      await expect(page.getByPlaceholder(/Write in Markdown/)).toHaveValue('base kept');

      // Restoring makes the editor dirty, so autosave sends it.
      await expect(page.getByTestId('save-state')).toHaveText(/^Saved · /, { timeout: 10_000 });
      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('base kept');
      expect(await editDrafts(page)).toEqual([]);
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('a draft the server already has is dropped without asking', async ({ page, request }) => {
    // Sighting 2: the save landed but the tab never heard back.
    const node = await createNode(request, `e2e draft landed ${Date.now()}`, { body: 'landed' });
    try {
      await page.goto('/');
      await expect(page.locator('main')).toBeVisible();
      await page.evaluate(
        ({ key, draft }) => localStorage.setItem(key, JSON.stringify(draft)),
        {
          key: `${DRAFT_PREFIX}${E2E_SUBJECT}:${node.id}`,
          draft: {
            title: node.title,
            body: 'landed',
            status: 'draft',
            kept_at: '2026-10-26T12:00:00Z',
            base: 0,
          },
        },
      );
      await openEditor(page, node.id, 'landed');
      await expect(page.getByTestId('draft-restore')).toHaveCount(0);
      await expect.poll(() => editDrafts(page)).toEqual([]);
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });
});
