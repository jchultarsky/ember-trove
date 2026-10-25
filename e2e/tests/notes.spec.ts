import { test, expect, createNode, createNote } from './fixtures';

/**
 * Notes on a node page. The node page's note panel had Edit but no Delete
 * (2026-10-22 bug) — the Notes feed had it. Both now share
 * `note_actions::delete_note_with_undo`: instant soft delete + Undo toast.
 */

test('deleting a note on its node page removes it, and Undo restores it', async ({ page, request }) => {
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
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('a collapsed note shows its cleaned-up first line and stays collapsed after reload', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e collapse node ${stamp}`);
  try {
    const note = await createNote(request, node.id, `## Meeting **notes** ${stamp}\n\nhidden detail ${stamp}`);
    const card = () => page.locator(`[data-note-id="${note.id}"]`);

    await page.goto(`/nodes/${node.id}`);
    await expect(card().getByText(`hidden detail ${stamp}`)).toBeVisible({ timeout: 15_000 });

    await card().getByRole('button', { name: 'Collapse note' }).click();
    const title = card().getByTestId('note-collapsed-title');
    await expect(title).toContainText(`Meeting notes ${stamp}`); // Markdown marks stripped
    await expect(card().getByText(`hidden detail ${stamp}`)).toHaveCount(0);

    // Persisted per account: still collapsed after a full reload.
    await page.reload();
    await expect(card().getByTestId('note-collapsed-title')).toBeVisible({ timeout: 15_000 });
    await expect(card().getByText(`hidden detail ${stamp}`)).toHaveCount(0);

    await card().getByRole('button', { name: 'Expand note' }).click();
    await expect(card().getByText(`hidden detail ${stamp}`)).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('Collapse all / Expand all toggles every note and persists', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e collapse-all node ${stamp}`);
  try {
    const a = await createNote(request, node.id, `first ${stamp}\nbody a ${stamp}`);
    const b = await createNote(request, node.id, `second ${stamp}\nbody b ${stamp}`);
    const titleOf = (id: string) => page.locator(`[data-note-id="${id}"]`).getByTestId('note-collapsed-title');

    await page.goto(`/nodes/${node.id}`);
    await expect(page.getByText(`body a ${stamp}`)).toBeVisible({ timeout: 15_000 });

    await page.getByRole('button', { name: 'Collapse all' }).click();
    await expect(titleOf(a.id)).toBeVisible();
    await expect(titleOf(b.id)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Expand all' })).toBeVisible();

    await page.reload();
    await expect(titleOf(a.id)).toBeVisible({ timeout: 15_000 });
    await expect(titleOf(b.id)).toBeVisible();

    await page.getByRole('button', { name: 'Expand all' }).click();
    await expect(page.getByText(`body a ${stamp}`)).toBeVisible();
    await expect(page.getByText(`body b ${stamp}`)).toBeVisible();
    await expect(titleOf(a.id)).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
