import { test, expect, createNode, type Page } from './fixtures';

/**
 * Every button has a real accessible name (#133). Icon-only buttons used to
 * be named by their Material Symbols glyph ("edit", "more_vert", "close")
 * or nothing at all. The glyph is now aria-hidden, so the title names the
 * button. This guards the main surfaces with real content on them.
 */

/** Buttons whose accessible name is empty or a bare glyph token. */
async function badButtonNames(page: Page): Promise<string[]> {
  const snapshot = await page.locator('body').ariaSnapshot();
  const bad: string[] = [];
  let seen = 0;
  for (const line of snapshot.split('\n')) {
    const m = line.match(/^\s*- button(?: "([^"]*)")?/);
    if (!m) continue;
    seen++;
    const name = m[1] ?? '';
    // A glyph name: one lowercase snake_case token ("edit", "more_vert").
    if (name.trim() === '' || /^[a-z][a-z0-9_]*$/.test(name.trim())) bad.push(line.trim());
  }
  // Guard against a vacuous pass (e.g. the snapshot format changing).
  expect(seen, 'buttons found in the accessibility snapshot').toBeGreaterThan(5);
  return bad;
}

test('My Day, Inbox and a full node page have no unnamed buttons', async ({ page, request }) => {
  const stamp = Date.now();
  // A node with every section populated: body, a task, a note.
  const node = await createNode(request, `e2e a11y node ${stamp}`, {
    node_type: 'project',
    body: 'Body text with a [[wiki link]].',
  });
  expect((await request.post(`/api/nodes/${node.id}/tasks`, { data: { title: `e2e a11y task ${stamp}` } })).ok()).toBeTruthy();
  expect(
    (await request.post(`/api/nodes/${node.id}/notes`, { data: { body: `e2e a11y note ${stamp}`, color: 'default' } })).ok(),
  ).toBeTruthy();
  const inbox = await request.post('/api/tasks', { data: { title: `e2e a11y inbox ${stamp}` } });
  expect(inbox.ok()).toBeTruthy();
  const inboxTask = await inbox.json();
  try {
    await page.goto('/tasks/inbox');
    await expect(page.locator(`[data-task-id="${inboxTask.id}"]`)).toBeVisible();
    expect(await badButtonNames(page)).toEqual([]);

    await page.goto(`/nodes/${node.id}`);
    await expect(page.getByTestId('node-tasks')).toBeVisible({ timeout: 15_000 });
    await page.getByRole('button', { name: /Connections/ }).click();
    expect(await badButtonNames(page)).toEqual([]);

    await page.goto('/tasks/my-day');
    await expect(page.locator('main')).toBeVisible();
    expect(await badButtonNames(page)).toEqual([]);
  } finally {
    await request.delete(`/api/tasks/${inboxTask.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});
