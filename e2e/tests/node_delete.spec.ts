import { test, expect, createNode, createNote, shareNode } from './fixtures';

/**
 * Deleting a node deletes its tasks, notes and files with it, other
 * accounts' included. The confirmation counts them first and keeps Delete
 * off until it has (#281, owner decision 2026-11-09).
 */

const OTHER_SUBJECT = 'e2e-other-user';

test("the delete confirmation counts what goes, other people's included", async ({
  page,
  request,
  otherRequest,
}) => {
  const node = await createNode(request, `e2e delete counts ${Date.now()}`, {
    node_type: 'project',
  });
  try {
    await shareNode(request, node.id, OTHER_SUBJECT, 'editor');
    await createNote(request, node.id, 'mine');
    await createNote(otherRequest, node.id, 'theirs');
    const task = await request.post(`/api/nodes/${node.id}/tasks`, {
      data: { title: 'e2e a task that goes too' },
    });
    expect(task.ok()).toBeTruthy();

    await page.goto(`/nodes/${node.id}`);
    // The header's icon button: its accessible name is its title.
    await page.getByRole('button', { name: 'Delete', exact: true }).first().click();

    const dialog = page.getByRole('alertdialog', { name: 'Confirm delete' });
    const details = dialog.getByTestId('delete-details');
    await expect(details).toContainText('This also deletes:');
    await expect(details).toContainText('1 task');
    await expect(details).toContainText('2 notes (1 from other people)');

    // Cancel keeps everything.
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();
    expect((await request.get(`/api/nodes/${node.id}`)).ok()).toBeTruthy();

    // Confirming deletes the node and what was counted.
    await page.getByRole('button', { name: 'Delete', exact: true }).first().click();
    await expect(details).toContainText('2 notes (1 from other people)');
    await dialog.getByRole('button', { name: 'Delete' }).click();
    await expect(page).toHaveURL(/\/nodes$/);
    // A deleted node answers like one that was never the caller's: 403.
    expect((await request.get(`/api/nodes/${node.id}`)).ok()).toBeFalsy();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('Delete stays off when the count fails', async ({ page, request }) => {
  const node = await createNode(request, `e2e delete uncounted ${Date.now()}`);
  try {
    await page.route('**/api/nodes/deletion-preview**', (route) =>
      route.fulfill({ status: 500, body: 'no' }),
    );
    await page.goto(`/nodes/${node.id}`);
    await page.getByRole('button', { name: 'Delete', exact: true }).first().click();

    const dialog = page.getByRole('alertdialog', { name: 'Confirm delete' });
    await expect(dialog.getByRole('alert')).toContainText('Couldn’t count what this deletes');
    await expect(dialog.getByRole('button', { name: 'Delete' })).toBeDisabled();
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    expect((await request.get(`/api/nodes/${node.id}`)).ok()).toBeTruthy();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
