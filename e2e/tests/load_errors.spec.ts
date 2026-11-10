import { test, expect, createNode } from './fixtures';

/**
 * A failed load is not an empty page (#281, finding 1.7). Views used to map
 * a fetch error to their empty state, so a 500 or a dropped connection read
 * as "No notes yet": lost data. They show what failed, and a retry.
 */

test('My Day says the load failed, and Try again loads it', async ({ page }) => {
  let fail = true;
  await page.route('**/api/my-day**', (route) =>
    fail ? route.fulfill({ status: 500, body: 'no' }) : route.continue(),
  );
  await page.goto('/tasks/my-day');
  const error = page.getByTestId('load-error').first();
  await expect(error).toContainText('Couldn’t load today’s tasks');
  // Not the empty day.
  await expect(page.getByText("Nothing on today's list")).toHaveCount(0);

  fail = false;
  await error.getByRole('button', { name: 'Try again' }).click();
  await expect(page.getByText(/Nothing on today|Tasks you're focused on today/).first()).toBeVisible();
  await expect(page.getByText('Couldn’t load today’s tasks')).toHaveCount(0);
});

test('the notes feed says the load failed, not "No notes yet"', async ({ page }) => {
  await page.route('**/api/notes/feed**', (route) => route.fulfill({ status: 500, body: 'no' }));
  await page.goto('/notes');
  await expect(page.getByTestId('load-error')).toContainText('Couldn’t load your notes');
  await expect(page.getByText('No notes yet')).toHaveCount(0);
});

test('templates say the load failed, not "No templates yet"', async ({ page }) => {
  await page.route('**/api/templates', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ status: 500, body: 'no' })
      : route.continue(),
  );
  await page.goto('/templates');
  await expect(page.getByTestId('load-error')).toContainText('Couldn’t load your templates');
  await expect(page.getByText('No templates yet')).toHaveCount(0);
});

test("a node's notes and tasks say the load failed", async ({ page, request }) => {
  const node = await createNode(request, `e2e load errors ${Date.now()}`, { node_type: 'project' });
  try {
    await page.route(`**/api/nodes/${node.id}/notes**`, (route) =>
      route.fulfill({ status: 500, body: 'no' }),
    );
    await page.route(`**/api/nodes/${node.id}/tasks**`, (route) =>
      route.fulfill({ status: 500, body: 'no' }),
    );
    await page.goto(`/nodes/${node.id}`);
    await expect(page.getByText('Couldn’t load this node’s tasks')).toBeVisible();
    await expect(page.getByText('No tasks yet.')).toHaveCount(0);
    // The notes section may start collapsed: open it if its toggle is there.
    const notes = page.getByText('Couldn’t load this node’s notes');
    if (!(await notes.isVisible())) {
      await page.locator('button[title="Expand notes"]').click();
    }
    await expect(notes).toBeVisible();
    await expect(page.getByText('No notes yet.')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
