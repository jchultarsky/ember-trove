import { test, expect, Page, APIRequestContext } from '@playwright/test';

/**
 * Notes on a node page. The node page's note panel had Edit but no Delete
 * (2026-10-22 bug) — the Notes feed had it. Both now share
 * `note_actions::delete_note_with_undo`: instant soft delete + Undo toast.
 */

function collectPageErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(String(err)));
  return errors;
}

async function createNode(request: APIRequestContext, title: string) {
  const resp = await request.post('/api/nodes', {
    data: { title, node_type: 'article', body: 'notes spec node', metadata: {}, status: 'draft' },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

test('deleting a note on its node page removes it, and Undo restores it', async ({ page, request }) => {
  const errors = collectPageErrors(page);
  const stamp = Date.now(); // unique per attempt, so retries never collide
  const node = await createNode(request, `e2e notes node ${stamp}`);
  try {
    const resp = await request.post(`/api/nodes/${node.id}/notes`, {
      data: { body: `e2e note to delete ${stamp}`, color: 'default' },
    });
    expect(resp.ok()).toBeTruthy();
    const note = await resp.json();
    const card = () => page.locator(`[data-note-id="${note.id}"]`);

    await page.goto(`/nodes/${node.id}`);
    await expect(card()).toBeVisible({ timeout: 15_000 });

    await card().getByRole('button', { name: 'Delete note' }).click();
    await expect(card()).toHaveCount(0);

    const toast = page.locator('[role="status"]');
    await expect(toast.getByText('Note deleted')).toBeVisible();
    await toast.getByRole('button', { name: 'Undo' }).click();
    await expect(card()).toBeVisible();

    expect(errors).toEqual([]);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
