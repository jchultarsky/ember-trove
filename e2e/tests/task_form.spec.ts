import { test, expect, createNode, localISO } from './fixtures';

/**
 * The shared add-task form (`NewTaskForm`): the ☀ toggle commits the new task
 * to My Day in the create request (#98 item 7) — no create-then-toggle step.
 * Titles are unique per attempt, so a crashed retry's leftovers never match.
 */

type Task = { id: string; title: string; focus_date: string | null };

test('node page: adding a task with ☀ on puts it in My Day', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e form node ${stamp}`, { node_type: 'project' });
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

// ── Due-date echo + past-date warning (#98 item 1) ──────────────────────────
//
// Fill date inputs as YYYY-MM-DD — never type into the native segments (a
// stray digit in the Month segment is exactly the bug this guards against).

test('editing a due date echoes it and warns before saving a past date', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e due node ${stamp}`, { node_type: 'project' });
  const title = `e2e due edit ${stamp}`;
  try {
    const created = await request.post(`/api/nodes/${node.id}/tasks`, {
      data: { title, due_date: localISO(3) },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();

    await page.goto(`/nodes/${node.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    // The glyph text ("edit") is the accessible name; target the title.
    await row.locator('button[title="Edit task"]').click();
    const field = row.getByTestId('due-date-field');
    const hint = field.getByTestId('due-hint');
    const warning = field.getByTestId('due-warning');

    // The saved date reads back in words; unchanged → no warning.
    await expect(hint).toContainText('in 3 days');
    await expect(warning).toBeEmpty();

    await field.locator('input[type="date"]').fill(localISO(9));
    await expect(hint).toContainText('in 9 days');
    await expect(warning).toBeEmpty();

    // A slip into the past is called out — but Save is not blocked.
    await field.locator('input[type="date"]').fill(localISO(-30));
    await expect(hint).toContainText('ago');
    await expect(warning).toHaveText('This date is in the past');
    await row.getByRole('button', { name: 'Save' }).click();
    await expect
      .poll(async () => {
        const tasks: Task[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
        return (tasks.find((t) => t.id === task.id) as Task & { due_date: string })?.due_date;
      })
      .toBe(localISO(-30));
  } finally {
    const tasks: Task[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
    for (const t of tasks) await request.delete(`/api/tasks/${t.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('the add-task form echoes the due date and warns on a past one', async ({ page }) => {
  await page.goto('/tasks/inbox');
  const field = page.getByTestId('new-task-form').getByTestId('due-date-field');
  const input = field.locator('input[type="date"]');
  await expect(field.getByTestId('due-hint')).toBeEmpty();

  await input.fill(localISO(1));
  await expect(field.getByTestId('due-hint')).toContainText('tomorrow');
  await expect(field.getByTestId('due-warning')).toBeEmpty();

  // New task: any past date is a change, so it warns.
  await input.fill(localISO(-1));
  await expect(field.getByTestId('due-hint')).toContainText('yesterday');
  await expect(field.getByTestId('due-warning')).toHaveText('This date is in the past');

  // Clearing removes both.
  await input.fill('');
  await expect(field.getByTestId('due-hint')).toBeEmpty();
  await expect(field.getByTestId('due-warning')).toBeEmpty();
});

// ── Row ☀ action (task_actions::toggle_my_day + MyDayButton) ────────────────

test('inbox: ☀ stays available while editing a task and applies immediately', async ({
  page,
  request,
}) => {
  const title = `e2e row sun ${Date.now()}`;
  const created = await request.post('/api/tasks', { data: { title } });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();
  try {
    await page.goto('/tasks/inbox');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Edit"]').click();
    // Edit hides Assign/Edit/Delete — but not ☀ (it isn't Save-gated).
    await expect(row.locator('button[title="Edit"]')).toHaveCount(0);
    const sun = row.getByRole('button', { name: 'Add to My Day' });
    await expect(sun).toHaveAttribute('aria-pressed', 'false');
    await sun.click();
    await expect(row.getByRole('button', { name: 'Remove from My Day' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await expect
      .poll(async () => {
        const inbox: Task[] = await (await request.get('/api/tasks/inbox')).json();
        return inbox.find((t) => t.id === task.id)?.focus_date ?? null;
      })
      .toBe(localISO(0));
  } finally {
    await request.delete(`/api/tasks/${task.id}`);
  }
});

test('a double Enter in the add-task form creates the task once', async ({ page, request }) => {
  const title = `e2e form double ${Date.now()}`;
  try {
    await page.goto('/tasks/inbox');
    const input = page.getByTestId('new-task-form').getByPlaceholder('Task title…');
    await input.fill(title);
    await input.press('Enter');
    await input.press('Enter');
    await expect(page.locator('[data-task-id]', { hasText: title })).toHaveCount(1);
    const inbox: Task[] = await (await request.get('/api/tasks/inbox')).json();
    expect(inbox.filter((t) => t.title === title)).toHaveLength(1);
  } finally {
    const inbox: Task[] = await (await request.get('/api/tasks/inbox')).json();
    for (const t of inbox.filter((t) => t.title === title)) await request.delete(`/api/tasks/${t.id}`);
  }
});

// ── Inline task editor (shared TaskEditDraft) ─────────────────────────────────
//
// Every list's editor opens on the task's saved values, Cancel discards ALL
// field edits (it used to reset only the title), and the node panel can edit
// recurrence like the other lists.

type FullTask = Task & { priority: string; recurrence: string | null };

test('node panel editor: opens on saved values, Cancel resets all, saves recurrence', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e editor node ${stamp}`, { node_type: 'project' });
  try {
    const created = await request.post(`/api/nodes/${node.id}/tasks`, {
      data: { title: `e2e editor ${stamp}`, priority: 'high', recurrence: 'weekly' },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();

    await page.goto(`/nodes/${node.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Edit task"]').click();
    const priority = row.locator('select[title="Priority"]');
    const repeat = row.locator('select[title="Recurrence"]');
    await expect(priority).toHaveValue('high');
    await expect(repeat).toHaveValue('weekly');

    // Abandon edits to every field: reopening shows the saved values.
    await priority.selectOption('low');
    await repeat.selectOption('daily');
    await row.getByRole('button', { name: 'Cancel' }).click();
    await row.locator('button[title="Edit task"]').click();
    await expect(priority).toHaveValue('high');
    await expect(repeat).toHaveValue('weekly');

    await repeat.selectOption('monthly');
    await row.getByRole('button', { name: 'Save' }).click();
    await expect
      .poll(async () => {
        const tasks: FullTask[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
        return tasks.find((t) => t.id === task.id)?.recurrence;
      })
      .toBe('monthly');
  } finally {
    const tasks: Task[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
    for (const t of tasks) await request.delete(`/api/tasks/${t.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('inbox editor: opens on saved values and Cancel resets every field', async ({
  page,
  request,
}) => {
  const title = `e2e inbox editor ${Date.now()}`;
  const created = await request.post('/api/tasks', {
    data: { title, priority: 'low', recurrence: 'yearly' },
  });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();
  try {
    await page.goto('/tasks/inbox');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Edit"]').click();
    const priority = row.locator('select[title="Priority"]');
    const repeat = row.locator('select[title="Recurrence"]');
    await expect(priority).toHaveValue('low');
    await expect(repeat).toHaveValue('yearly');
    // One label per rule everywhere (this list used to say "Biweekly").
    await expect(repeat.locator('option[value="biweekly"]')).toHaveText('Every 2 weeks');

    await priority.selectOption('high');
    await repeat.selectOption('');
    await row.getByRole('button', { name: 'Cancel' }).click();
    await row.locator('button[title="Edit"]').click();
    await expect(priority).toHaveValue('low');
    await expect(repeat).toHaveValue('yearly');
  } finally {
    await request.delete(`/api/tasks/${task.id}`);
  }
});

test('a resized task editor reopens at its new height (same session)', async ({
  page,
  request,
}) => {
  const title = `e2e editor height ${Date.now()}`;
  const created = await request.post('/api/tasks', { data: { title } });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();
  try {
    await page.goto('/tasks/inbox');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Edit"]').click();
    const editor = row.locator('textarea');
    // A drag of the native resize handle ends in a mouseup on the textarea.
    await editor.evaluate((el) => {
      (el as HTMLElement).style.height = '160px';
      el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    });
    await row.getByRole('button', { name: 'Cancel' }).click();
    // Regression: the height map was loaded once at page start, so the
    // reopened editor snapped back to the old height.
    await row.locator('button[title="Edit"]').click();
    await expect.poll(() => editor.evaluate((el) => (el as HTMLElement).offsetHeight)).toBe(160);
  } finally {
    await request.delete(`/api/tasks/${task.id}`);
  }
});
