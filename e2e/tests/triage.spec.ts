import { test, expect, APIRequestContext } from '@playwright/test';

/**
 * Inbox triage ("Process" mode) — one task at a time, single-key decisions.
 * Tasks are created via the API (bypass-authenticated) BEFORE entering
 * triage: the working set is a snapshot taken on mount.
 */

async function createTask(request: APIRequestContext, title: string): Promise<string> {
  const resp = await request.post('/api/tasks', { data: { title } });
  expect(resp.ok()).toBeTruthy();
  return (await resp.json()).id;
}

async function deleteTask(request: APIRequestContext, id: string) {
  await request.delete(`/api/tasks/${id}`);
}

// Triage snapshots the whole open inbox on entry, so these tests need to own
// it. The e2e database is ephemeral and this suite is its only client —
// clearing leftovers from earlier specs keeps the working set deterministic.
test.beforeEach(async ({ request }) => {
  const inbox: { id: string; status: string }[] =
    await (await request.get('/api/tasks/inbox')).json();
  for (const t of inbox) {
    if (t.status !== 'done' && t.status !== 'cancelled') {
      await deleteTask(request, t.id);
    }
  }
});

test('t commits the task to today and finishes triage', async ({ page, request }) => {
  const title = `e2e triage-today ${Date.now()}`;
  const id = await createTask(request, title);

  try {
    await page.goto('/tasks/inbox');
    await page.getByRole('button', { name: /Process/ }).click();
    // Title assertions scope to the card — the same text exists in the
    // CSS-hidden inbox list behind it (strict-mode collision otherwise).
    const card = page.getByTestId('triage-card');
    await expect(card.getByText(title)).toBeVisible();

    await page.keyboard.press('t');
    await expect(page.locator('[role="status"]').getByText('Added to today')).toBeVisible();
    // Single-task set → finishing toast + exit back to the inbox list.
    await expect(page.locator('[role="status"]').getByText(/Inbox processed/)).toBeVisible();
    await expect(page.getByRole('button', { name: /Process/ })).toBeVisible();

    // Server state: the task is now in My Day.
    // MyDayTask serde-flattens the task — the wire shape is flat.
    const myDay = await (await request.get('/api/my-day')).json();
    expect(myDay.some((t: { id: string }) => t.id === id)).toBe(true);
  } finally {
    await deleteTask(request, id);
  }
});

test('s schedules a due date; a attaches to a node', async ({ page, request }) => {
  const stamp = Date.now();
  const scheduleTitle = `e2e triage-schedule ${stamp}`;
  const attachTitle = `e2e triage-attach ${stamp}`;
  const nodeTitle = `e2e triage-node ${stamp}`;

  const scheduleId = await createTask(request, scheduleTitle);
  const attachId = await createTask(request, attachTitle);
  const nodeResp = await request.post('/api/nodes', {
    data: { title: nodeTitle, node_type: 'project', body: '', metadata: {}, status: 'draft' },
  });
  expect(nodeResp.ok()).toBeTruthy();
  const nodeId = (await nodeResp.json()).id;

  const due = new Date(Date.now() + 7 * 24 * 3600 * 1000).toISOString().slice(0, 10);

  try {
    await page.goto('/tasks/inbox');
    await page.getByRole('button', { name: /Process/ }).click();
    const card = page.getByTestId('triage-card');
    await expect(card.getByText(/^1 of 2$/)).toBeVisible();

    // Tasks are processed oldest-first (creation order): schedule first.
    await expect(card.getByText(scheduleTitle)).toBeVisible();
    await page.keyboard.press('s');
    const dateInput = card.locator('input[type="date"]');
    await expect(dateInput).toBeVisible();
    await dateInput.fill(due);
    await dateInput.press('Enter');
    await expect(page.locator('[role="status"]').getByText('Due date set')).toBeVisible();

    // Next task slides in: attach via the picker.
    await expect(card.getByText(attachTitle)).toBeVisible();
    await page.keyboard.press('a');
    const picker = page.getByPlaceholder('Attach to node…');
    await expect(picker).toBeVisible();
    await picker.fill(nodeTitle);
    // Debounced search → first (highlighted) result picked with Enter.
    await expect(page.getByRole('button', { name: nodeTitle })).toBeVisible();
    await picker.press('Enter');
    await expect(page.locator('[role="status"]').getByText('Attached to node')).toBeVisible();
    await expect(page.locator('[role="status"]').getByText(/Inbox processed/)).toBeVisible();

    // Server state for both decisions.
    const inbox = await (await request.get('/api/tasks/inbox')).json();
    const scheduled = inbox.find((t: { id: string }) => t.id === scheduleId);
    expect(scheduled?.due_date).toBe(due);
    const nodeTasks = await (await request.get(`/api/nodes/${nodeId}/tasks`)).json();
    expect(nodeTasks.some((t: { id: string }) => t.id === attachId)).toBe(true);
  } finally {
    await deleteTask(request, scheduleId);
    await deleteTask(request, attachId);
    await request.delete(`/api/nodes/${nodeId}`);
  }
});

test('j skips (wrapping) and Esc exits without changes', async ({ page, request }) => {
  const stamp = Date.now();
  const first = `e2e triage-skip-a ${stamp}`;
  const second = `e2e triage-skip-b ${stamp}`;
  const idA = await createTask(request, first);
  const idB = await createTask(request, second);

  try {
    await page.goto('/tasks/inbox');
    await page.getByRole('button', { name: /Process/ }).click();
    const card = page.getByTestId('triage-card');
    await expect(card.getByText(first)).toBeVisible();

    await page.keyboard.press('j');
    await expect(card.getByText(second)).toBeVisible();
    await page.keyboard.press('j'); // wraps back around
    await expect(card.getByText(first)).toBeVisible();

    await page.keyboard.press('Escape');
    // Back on the list, both tasks untouched.
    await expect(page.getByRole('button', { name: /Process/ })).toBeVisible();
    await expect(page.locator('[data-task-id]', { hasText: first })).toBeVisible();
    await expect(page.locator('[data-task-id]', { hasText: second })).toBeVisible();
  } finally {
    await deleteTask(request, idA);
    await deleteTask(request, idB);
  }
});
