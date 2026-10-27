import { test, expect, createNode, type Page } from './fixtures';

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
