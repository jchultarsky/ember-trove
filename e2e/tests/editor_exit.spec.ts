import { test, expect, createNode, type Page } from './fixtures';

/**
 * #197: the node editor autosaves, so its exit buttons say what they do.
 * Edit mode has "Done" (keeps the edits, which are already saving) and,
 * once something changed, "Revert" (back to the node as it was opened).
 * There is no "Cancel" there: it used to save the edits anyway. Create mode
 * keeps "Create" and "Cancel", because nothing reaches the server until Create.
 */

async function openEditor(page: Page, id: string) {
  await page.goto(`/nodes/${id}/edit`);
  const editor = page.getByPlaceholder(/Write in Markdown/);
  await expect(editor).toBeVisible();
  return editor;
}

test.describe('editor exit buttons (#197)', () => {
  test('edit mode: Done keeps the edits, and there is no Cancel', async ({ page, request }) => {
    const node = await createNode(request, `e2e exit done ${Date.now()}`, { body: 'as opened' });
    try {
      const editor = await openEditor(page, node.id);
      await expect(editor).toHaveValue('as opened');
      await expect(page.getByRole('button', { name: 'Done', exact: true })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Cancel' })).toHaveCount(0);
      // Nothing to revert before an edit.
      await expect(page.getByRole('button', { name: 'Revert' })).toHaveCount(0);

      await editor.fill('as opened, then edited');
      // Done right away, inside the 2 s debounce: it must still save.
      await page.getByRole('button', { name: 'Done', exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('as opened, then edited');
      // The edit's debounce timer fires after the editor is gone. It must not
      // touch disposed signals: a panic is a pageerror, which fails the test.
      await page.waitForTimeout(2_500);
      await expect(page.locator('main')).toBeVisible();
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('edit mode: Revert restores the node as opened; the edit stays in History', async ({ page, request }) => {
    const node = await createNode(request, `e2e exit revert ${Date.now()}`, { body: 'as opened' });
    try {
      const editor = await openEditor(page, node.id);
      await editor.fill('an edit I regret');
      // Let autosave put the edit on the server first: the risky case.
      await expect(page.getByText(/^Saved · /)).toBeVisible({ timeout: 10_000 });

      const revert = page.getByRole('button', { name: 'Revert' });
      await expect(revert).toBeVisible();
      await revert.click();
      await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));

      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('as opened');
      // The reverted text is recoverable from History.
      const versions = await (await request.get(`/api/nodes/${node.id}/versions`)).json();
      expect(versions.map((v: { body: string }) => v.body)).toContain('an edit I regret');
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('edit mode: Revert disappears when the text is back as opened', async ({ page, request }) => {
    const node = await createNode(request, `e2e exit back ${Date.now()}`, { body: 'as opened' });
    try {
      const editor = await openEditor(page, node.id);
      await editor.fill('changed');
      await expect(page.getByRole('button', { name: 'Revert' })).toBeVisible();
      await editor.fill('as opened');
      await expect(page.getByRole('button', { name: 'Revert' })).toHaveCount(0);
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('create mode: Create and Cancel, no Done or Revert', async ({ page }) => {
    await page.goto('/nodes/new');
    await expect(page.getByPlaceholder(/Write in Markdown/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Create', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Done', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Revert' })).toHaveCount(0);

    // Cancel inside the draft debounce: the forgotten timer must not panic
    // on the disposed editor (a pageerror fails the test).
    await page.getByPlaceholder('Node title…').fill(`e2e exit cancel ${Date.now()}`);
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(page).toHaveURL(/\/nodes$/);
    await page.waitForTimeout(2_500);
    await expect(page.locator('main')).toBeVisible();
  });
});
