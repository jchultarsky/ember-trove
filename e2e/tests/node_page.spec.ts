import { test, expect, createNode, createNote, type Page } from './fixtures';

/**
 * Node page friction (#123): node-scoped "Jump to" actions, the scroll
 * position surviving a task/note add (the page used to unmount for a frame
 * under one Suspense), and Escape in an editor offering Undo instead of
 * silently discarding the edit.
 */

const LONG_BODY = Array.from({ length: 80 }, (_, i) => `Paragraph ${i} of a long node.`).join('\n\n');

/** The node page's scroll container. Notes are on every kind (§3.2): Tasks
 * and People are only on Projects (#216). */
const scroller = (page: Page) => page.getByTestId('node-notes').locator('xpath=..');

async function gotoNode(page: Page, id: string) {
  await page.goto(`/nodes/${id}`);
  await expect(page.getByTestId('node-notes')).toBeVisible({ timeout: 15_000 });
}

/** Open the header's section outline and jump to `name`. */
async function jumpTo(page: Page, name: string) {
  await page.getByRole('button', { name: 'Sections', exact: true }).click();
  await page.getByRole('menu', { name: 'Sections' }).getByRole('menuitem', { name: new RegExp(name) }).click();
}

test('the section outline reveals and focuses this node’s sections', async ({ page, request }) => {
  const node = await createNode(request, `e2e jump node ${Date.now()}`, {
    body: LONG_BODY,
    node_type: 'project',
  });
  try {
    await gotoNode(page, node.id);
    const tasks = page.getByTestId('node-tasks');
    await expect(tasks).not.toBeInViewport();
    await jumpTo(page, 'Tasks');
    await expect(tasks).toBeFocused();
    await expect(tasks).toBeInViewport();

    // A collapsed Notes section is expanded by the jump.
    await page.locator('button[title="Collapse notes"]').click();
    await expect(page.locator('button[title="Expand notes"]')).toBeVisible();
    await scroller(page).evaluate((el) => (el.scrollTop = 0));
    await jumpTo(page, 'Notes');
    const notes = page.getByTestId('node-notes');
    await expect(notes).toBeFocused();
    await expect(notes).toBeInViewport();
    await expect(page.locator('button[title="Collapse notes"]')).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('every section is in the outline and one shared look (#131)', async ({ page, request }) => {
  const node = await createNode(request, `e2e outline node ${Date.now()}`, {
    body: LONG_BODY,
    node_type: 'project',
  });
  try {
    await gotoNode(page, node.id);
    await page.getByRole('button', { name: 'Sections', exact: true }).click();
    const items = page.getByRole('menu', { name: 'Sections' }).getByRole('menuitem');
    await expect(items).toHaveText([
      /Tasks/, /Notes/, /People/, /External Links/, /Connections/, /Local Graph/,
      /Linked Here/, /Attachments/, /Sharing/, /Version History/, /Activity/,
    ]);
    // Jumping to a collapsed section opens and focuses it.
    await items.filter({ hasText: 'Version History' }).click();
    const versions = page.getByTestId('node-versions');
    await expect(versions).toBeFocused();
    await expect(versions.getByRole('button', { name: /Version History/ })).toHaveAttribute('aria-expanded', 'true');
    // One heading style: every section heading is an h2, named like the outline.
    // Public Links went and People came (#147).
    await expect(page.locator('[data-testid^="node-"] h2')).toHaveCount(11);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('an Article has no Tasks or People section, and the outline follows (#216)', async ({
  page,
  request,
}) => {
  const node = await createNode(request, `e2e article sections ${Date.now()}`);
  try {
    await gotoNode(page, node.id);
    await expect(page.getByTestId('node-tasks')).toHaveCount(0);
    await expect(page.getByTestId('node-people')).toHaveCount(0);
    await expect(page.locator('button[title="Add task"]')).toHaveCount(0);
    await page.getByRole('button', { name: 'Sections', exact: true }).click();
    const items = page.getByRole('menu', { name: 'Sections' }).getByRole('menuitem');
    await expect(items).toHaveText([
      /Notes/, /External Links/, /Connections/, /Local Graph/,
      /Linked Here/, /Attachments/, /Sharing/, /Version History/, /Activity/,
    ]);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('adding a task or a note keeps the scroll position', async ({ page, request }) => {
  const stamp = Date.now();
  // A Project: only Projects take tasks (§3.2, #216).
  const node = await createNode(request, `e2e scroll node ${stamp}`, {
    body: LONG_BODY,
    node_type: 'project',
  });
  try {
    await gotoNode(page, node.id);
    await jumpTo(page, 'Tasks');
    const box = scroller(page);
    // Regression: the whole page unmounted for a frame (one Suspense around
    // every panel), so the container — and its scrollTop — was replaced.
    await box.evaluate((el) => ((el as HTMLElement & { __probe?: boolean }).__probe = true));
    const before = await box.evaluate((el) => el.scrollTop);
    expect(before).toBeGreaterThan(500);

    await page.locator('button[title="Add task"]').click();
    const form = page.getByTestId('new-task-form');
    await form.getByPlaceholder('Task title…').fill(`e2e scroll task ${stamp}`);
    await form.getByPlaceholder('Task title…').press('Enter');
    await expect(page.getByTestId('node-tasks').getByText(`e2e scroll task ${stamp}`)).toBeVisible();
    await page.waitForTimeout(300); // let any refetch settle
    expect(await box.evaluate((el) => (el as HTMLElement & { __probe?: boolean }).__probe)).toBe(true);
    expect(Math.abs((await box.evaluate((el) => el.scrollTop)) - before)).toBeLessThan(150);

    const notesBefore = await box.evaluate((el) => el.scrollTop);
    await page.locator('button[title="Add note"]').click();
    await page.getByPlaceholder('Write a note… (Ctrl+Enter to save)').fill(`e2e scroll note ${stamp}`);
    await page.locator('button[title="Save note"]').click();
    await expect(page.getByTestId('node-notes').getByText(`e2e scroll note ${stamp}`)).toBeVisible();
    await page.waitForTimeout(300);
    expect(await box.evaluate((el) => (el as HTMLElement & { __probe?: boolean }).__probe)).toBe(true);
    expect(Math.abs((await box.evaluate((el) => el.scrollTop)) - notesBefore)).toBeLessThan(300);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('Escape in the task editor offers Undo for a changed edit', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e escape node ${stamp}`, { node_type: 'project' });
  try {
    const created = await request.post(`/api/nodes/${node.id}/tasks`, {
      data: { title: `e2e escape original ${stamp}` },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();
    await gotoNode(page, node.id);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    const toast = page.locator('[role="status"]');

    // No change: Escape just closes, silently.
    await row.locator('button[title="Edit task"]').click();
    await row.locator('textarea').press('Escape');
    // Regression: the Escape also reached the window dispatcher and meant
    // "back to /nodes", so a stray Escape left the node.
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    await expect(row.locator('textarea')).toHaveCount(0);
    await expect(toast.getByText('Edit discarded')).toHaveCount(0);

    // A change: Escape closes, but Undo brings the typed text back.
    await row.locator('button[title="Edit task"]').click();
    await row.locator('textarea').fill(`e2e escape changed ${stamp}`);
    await row.locator('textarea').press('Escape');
    await expect(row.locator('textarea')).toHaveCount(0);
    await expect(row.getByText(`e2e escape original ${stamp}`)).toBeVisible();
    await toast.getByRole('button', { name: 'Undo' }).click();
    await expect(row.locator('textarea')).toHaveValue(`e2e escape changed ${stamp}`);
  } finally {
    const tasks = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
    for (const t of tasks) await request.delete(`/api/tasks/${t.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('Escape in the note editor offers Undo for a changed edit', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e note escape node ${stamp}`);
  try {
    const note = await createNote(request, node.id, `e2e note original ${stamp}`);
    await gotoNode(page, node.id);
    const card = page.locator(`[data-note-id="${note.id}"]`);
    await card.locator('button[title="Edit note"]').click();
    const editor = card.getByPlaceholder('Note…');
    await editor.fill(`e2e note changed ${stamp}`);
    await editor.press('Escape');
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    await expect(editor).toHaveCount(0);
    await expect(card.getByText(`e2e note original ${stamp}`)).toBeVisible();
    await page.locator('[role="status"]').getByRole('button', { name: 'Undo' }).click();
    await expect(card.getByPlaceholder('Note…')).toHaveValue(`e2e note changed ${stamp}`);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('a long body gets a contents list that jumps to its headings (#185)', async ({ page, request }) => {
  const filler = Array.from({ length: 30 }, (_, i) => `Filler ${i}.`).join('\n\n');
  const body = `## Alpha part\n\n${filler}\n\n### Beta detail\n\n${filler}\n\n## Omega part\n\n${filler}`;
  const long = await createNode(request, `e2e body outline ${Date.now()}`, { body });
  const short = await createNode(request, `e2e short outline ${Date.now()}`, {
    body: '## One\n\ntext\n\n## Two\n\ntext',
  });
  try {
    await gotoNode(page, long.id);
    const contents = page.getByRole('navigation', { name: 'Contents' });
    await expect(contents.getByRole('button')).toHaveText(['Alpha part', 'Beta detail', 'Omega part']);
    const omega = page.getByRole('heading', { name: 'Omega part', level: 2 });
    await expect(omega).not.toBeInViewport();
    await contents.getByRole('button', { name: 'Omega part' }).click();
    await expect(omega).toBeInViewport();

    // Two headings: no outline.
    await gotoNode(page, short.id);
    await expect(page.getByRole('heading', { name: 'Two', level: 2 })).toBeVisible();
    await expect(page.getByTestId('body-outline')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${long.id}`);
    await request.delete(`/api/nodes/${short.id}`);
  }
});
