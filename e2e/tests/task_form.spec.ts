import { test, expect, type APIRequestContext } from '@playwright/test';

/**
 * The shared add-task form (`NewTaskForm`): the ☀ toggle commits the new task
 * to My Day in the create request (#98 item 7) — no create-then-toggle step.
 * Titles are unique per attempt, so a crashed retry's leftovers never match.
 */

function localISO(offsetDays: number): string {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`;
}

type Task = { id: string; title: string; focus_date: string | null };

async function createNode(request: APIRequestContext, title: string) {
  const resp = await request.post('/api/nodes', {
    data: { title, node_type: 'project', body: '', metadata: {}, status: 'draft' },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

test('node page: adding a task with ☀ on puts it in My Day', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e form node ${stamp}`);
  const title = `e2e form my-day ${stamp}`;
  try {
    await page.goto(`/nodes/${node.id}`);
    await page.locator('button[title="Add task"]').first().click();
    const form = page.getByTestId('new-task-form');
    await form.getByPlaceholder('Task title…').fill(title);
    const sun = form.getByRole('button', { name: 'Add to My Day' });
    await sun.click();
    await expect(sun).toHaveAttribute('aria-pressed', 'true');
    await form.getByPlaceholder('Task title…').press('Enter');

    await expect
      .poll(async () => {
        const tasks: Task[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
        return tasks.find((t) => t.title === title)?.focus_date ?? null;
      })
      .toBe(localISO(0));

    await page.goto('/tasks/my-day');
    await expect(page.locator('[data-task-id]', { hasText: title })).toBeVisible();
  } finally {
    const tasks: Task[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
    for (const t of tasks) await request.delete(`/api/tasks/${t.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('inbox: ☀ is off by default and resets after adding', async ({ page, request }) => {
  const stamp = Date.now();
  const first = `e2e form plain ${stamp}`;
  try {
    await page.goto('/tasks/inbox');
    const form = page.getByTestId('new-task-form');
    const sun = form.getByRole('button', { name: 'Add to My Day' });
    await expect(sun).toHaveAttribute('aria-pressed', 'false');

    // On for one task, then the form resets to off for the next.
    await sun.click();
    await form.getByPlaceholder('Task title…').fill(first);
    await form.getByPlaceholder('Task title…').press('Enter');
    await expect(page.locator('[data-task-id]', { hasText: first })).toBeVisible();
    await expect(sun).toHaveAttribute('aria-pressed', 'false');

    const second = `e2e form plain-2 ${stamp}`;
    await form.getByPlaceholder('Task title…').fill(second);
    await form.getByPlaceholder('Task title…').press('Enter');
    await expect(page.locator('[data-task-id]', { hasText: second })).toBeVisible();

    const inbox: Task[] = await (await request.get('/api/tasks/inbox')).json();
    expect(inbox.find((t) => t.title === first)?.focus_date).toBe(localISO(0));
    expect(inbox.find((t) => t.title === second)?.focus_date).toBeNull();
  } finally {
    const inbox: Task[] = await (await request.get('/api/tasks/inbox')).json();
    for (const t of inbox.filter((t) => t.title.includes(`${stamp}`))) {
      await request.delete(`/api/tasks/${t.id}`);
    }
  }
});
