import {
  test,
  expect,
  createNode,
  localISO,
  type APIRequestContext,
  type Locator,
  type Page,
} from './fixtures';

/**
 * The shared add-task form (`NewTaskForm`): the ☀ toggle commits the new task
 * to My Day in the create request (#98 item 7) — no create-then-toggle step.
 * Titles are unique per attempt, so a crashed retry's leftovers never match.
 */

type Task = { id: string; title: string; focus_date: string | null };

/**
 * Run `body` with a fresh Project, then delete its tasks and the Project.
 * (These tests used the Inbox's form and rows until S4c, #240: a task now
 * lives in a Project.)
 */
async function withProject(
  request: APIRequestContext,
  name: string,
  body: (node: { id: string }) => Promise<void>,
) {
  const node = await createNode(request, `${name} ${Date.now()}`, { node_type: 'project' });
  try {
    await body(node);
  } finally {
    const tasks: Task[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
    for (const t of tasks) await request.delete(`/api/tasks/${t.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
}

/**
 * After a refetch's response: let the app read it and re-render (the
 * response resolves on its headers, before the WASM has the body).
 */
async function settled(page: Page) {
  await page.waitForLoadState('networkidle');
  await page.evaluate(
    () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null)))),
  );
}

const nodeTasks = async (request: APIRequestContext, nodeId: string): Promise<Task[]> =>
  (await request.get(`/api/nodes/${nodeId}/tasks`)).json();

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

test('node page: ☀ is off by default for each new task', async ({ page, request }) => {
  await withProject(request, 'e2e form plain node', async (node) => {
    const stamp = Date.now();
    const first = `e2e form plain ${stamp}`;
    await page.goto(`/nodes/${node.id}`);
    await page.locator('button[title="Add task"]').first().click();
    const form = page.getByTestId('new-task-form');
    const sun = form.getByRole('button', { name: 'Add to My Day' });
    await expect(sun).toHaveAttribute('aria-pressed', 'false');

    // On for one task, then the form is off again for the next.
    await sun.click();
    await form.getByPlaceholder('Task title…').fill(first);
    await form.getByPlaceholder('Task title…').press('Enter');
    await expect(page.locator('[data-task-id]', { hasText: first })).toBeVisible();

    const second = `e2e form plain-2 ${stamp}`;
    await page.locator('button[title="Add task"]').first().click();
    await expect(sun).toHaveAttribute('aria-pressed', 'false');
    await form.getByPlaceholder('Task title…').fill(second);
    await form.getByPlaceholder('Task title…').press('Enter');
    await expect(page.locator('[data-task-id]', { hasText: second })).toBeVisible();

    const tasks = await nodeTasks(request, node.id);
    expect(tasks.find((t) => t.title === first)?.focus_date).toBe(localISO(0));
    expect(tasks.find((t) => t.title === second)?.focus_date).toBeNull();
  });
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

test('the add-task form echoes the due date and warns on a past one', async ({ page, request }) => {
  await withProject(request, 'e2e form due node', async (node) => {
  await page.goto(`/nodes/${node.id}`);
  await page.locator('button[title="Add task"]').first().click();
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
});

// ── Row ☀ action (task_actions::toggle_my_day + MyDayButton) ────────────────

test('node panel: ☀ stays available while editing a task and applies immediately', async ({
  page,
  request,
}) => {
  await withProject(request, 'e2e row sun node', async (node) => {
    const title = `e2e row sun ${Date.now()}`;
    const created = await request.post(`/api/nodes/${node.id}/tasks`, { data: { title } });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();
    await page.goto(`/nodes/${node.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Edit task"]').click();
    // ☀ isn't Save-gated: it acts while the editor is open.
    const editor = row.getByTestId('task-editor');
    await expect(editor).toBeVisible();
    // An unsaved edit. The refetch that ☀ causes rebuilt the row, which
    // closed the editor and lost the text (#250).
    await editor.locator('textarea').fill(`${title} reworded`);
    const refetched = page.waitForResponse(
      (r) => r.request().method() === 'GET' && r.url().endsWith(`/api/nodes/${node.id}/tasks`),
    );
    const sun = row.getByRole('button', { name: 'Add to My Day' });
    await expect(sun).toHaveAttribute('aria-pressed', 'false');
    await sun.click();
    await expect(row.getByRole('button', { name: 'Remove from My Day' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await expect
      .poll(async () => {
        const tasks = await nodeTasks(request, node.id);
        return tasks.find((t) => t.id === task.id)?.focus_date ?? null;
      })
      .toBe(localISO(0));
    await refetched;
    await settled(page);
    // The editor is still open, with the text as typed, and nothing saved.
    await expect(editor.locator('textarea')).toHaveValue(`${title} reworded`);
    expect((await nodeTasks(request, node.id)).find((t) => t.id === task.id)?.title).toBe(title);
    await editor.getByRole('button', { name: 'Save' }).click();
    await expect
      .poll(async () => (await nodeTasks(request, node.id)).find((t) => t.id === task.id)?.title)
      .toBe(`${title} reworded`);
  });
});

test('My Day: an open edit survives a quick capture reloading the list', async ({
  page,
  request,
}) => {
  await withProject(request, 'e2e myday keep node', async (node) => {
    const title = `e2e myday keep ${Date.now()}`;
    try {
      const created = await request.post(`/api/nodes/${node.id}/tasks`, {
        data: { title, focus_date: localISO(0) },
      });
      expect(created.ok()).toBeTruthy();
      const task = await created.json();
      await page.goto('/tasks/my-day');
      const row = page.locator(`[data-task-id="${task.id}"]`);
      await row.getByRole('button', { name: 'Edit task' }).click();
      const editor = row.getByTestId('task-editor');
      await editor.locator('textarea').fill(`${title} reworded`);

      // A quick capture bumps the task refresh: both zones reload and every
      // row is rebuilt (the task sits in Today, from /api/my-day).
      const refetched = Promise.all([
        page.waitForResponse((r) => r.request().method() === 'GET' && r.url().endsWith('/api/tasks/all')),
        page.waitForResponse((r) => r.request().method() === 'GET' && r.url().includes('/api/my-day?')),
      ]);
      await page.locator('main').click({ position: { x: 5, y: 5 } });
      await page.keyboard.press('n');
      await page.getByPlaceholder(/What’s on your mind/).fill(`e2e cap ${title}`);
      await page.keyboard.press('ControlOrMeta+Enter');
      await refetched;
      await settled(page);

      await expect(editor.locator('textarea')).toHaveValue(`${title} reworded`);
      // Cancel still goes back to the saved title, and reopening shows it.
      await editor.getByRole('button', { name: 'Cancel' }).click();
      await row.getByRole('button', { name: 'Edit task' }).click();
      await expect(editor.locator('textarea')).toHaveValue(title);
    } finally {
      const captures: { id: string; body: string }[] = await (
        await request.get('/api/captures')
      ).json();
      for (const c of captures.filter((c) => c.body === `e2e cap ${title}`)) {
        await request.delete(`/api/captures/${c.id}`);
      }
    }
  });
});

test('My Day: an edit survives the task moving to the backlog', async ({ page, request }) => {
  // Review of #250: the two zones reload apart, so a per-row copy of the
  // draft raced the move.
  await withProject(request, 'e2e myday move node', async (node) => {
    const title = `e2e myday move ${Date.now()}`;
    const created = await request.post(`/api/nodes/${node.id}/tasks`, {
      data: { title, focus_date: localISO(0) },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();
    await page.goto('/tasks/my-day');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.getByRole('button', { name: 'Edit task' }).click();
    await row.getByTestId('task-editor').locator('textarea').fill(`${title} moved`);
    await row.locator('button[title="Remove from today (back to backlog)"]').click();
    await expect
      .poll(async () => (await nodeTasks(request, node.id)).find((t) => t.id === task.id)?.focus_date ?? null)
      .toBeNull();
    await settled(page);
    // One row, in the backlog, still editing, with the text as typed.
    await expect(row).toHaveCount(1);
    await expect(row.getByTestId('task-editor').locator('textarea')).toHaveValue(`${title} moved`);
  });
});

test('a double Enter in the add-task form creates the task once', async ({ page, request }) => {
  await withProject(request, 'e2e form double node', async (node) => {
    const title = `e2e form double ${Date.now()}`;
    await page.goto(`/nodes/${node.id}`);
    await page.locator('button[title="Add task"]').first().click();
    const input = page.getByTestId('new-task-form').getByPlaceholder('Task title…');
    await input.fill(title);
    // The second Enter lands before the first create returns (the form
    // closes after a create, so it can't come later).
    await input.evaluate((el) => {
      for (let i = 0; i < 2; i++) {
        el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      }
    });
    await expect(page.locator('[data-task-id]', { hasText: title })).toHaveCount(1);
    // Let any second request land before counting.
    await page.waitForTimeout(500);
    const tasks = await nodeTasks(request, node.id);
    expect(tasks.filter((t) => t.title === title)).toHaveLength(1);
  });
});

// ── Inline task editor (shared TaskEditDraft) ─────────────────────────────────
//
// Every list's editor opens on the task's saved values, Cancel discards ALL
// field edits (it used to reset only the title), and the node panel can edit
// recurrence like the other lists.

type FullTask = Task & { priority: string; recurrence: string | null };

/** The editor's priority radio group (`PriorityPicker`, #132). */
const priorityOf = (scope: Locator) => scope.getByRole('radiogroup', { name: 'Priority' });
const expectPriority = (scope: Locator, label: 'Low' | 'Medium' | 'High') =>
  expect(priorityOf(scope).getByRole('radio', { name: label })).toHaveAttribute('aria-checked', 'true');

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
    const repeat = row.locator('select[title="Recurrence"]');
    await expectPriority(row, 'High');
    await expect(repeat).toHaveValue('weekly');

    // Abandon edits to every field: reopening shows the saved values.
    await priorityOf(row).getByRole('radio', { name: 'Low' }).click();
    await expectPriority(row, 'Low');
    await repeat.selectOption('daily');
    await row.getByRole('button', { name: 'Cancel' }).click();
    await row.locator('button[title="Edit task"]').click();
    await expectPriority(row, 'High');
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

test('My Day editor: opens on saved values and Cancel resets every field', async ({
  page,
  request,
}) => {
  await withProject(request, 'e2e myday editor node', async (node) => {
    const title = `e2e myday editor ${Date.now()}`;
    const created = await request.post(`/api/nodes/${node.id}/tasks`, {
      data: { title, priority: 'low', recurrence: 'yearly', focus_date: localISO(0) },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();
    await page.goto('/tasks/my-day');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.getByRole('button', { name: 'Edit task' }).click();
    const repeat = row.locator('select[title="Recurrence"]');
    await expectPriority(row, 'Low');
    await expect(repeat).toHaveValue('yearly');
    // One label per rule everywhere (this list used to say "Biweekly").
    await expect(repeat.locator('option[value="biweekly"]')).toHaveText('Every 2 weeks');

    await priorityOf(row).getByRole('radio', { name: 'High' }).click();
    await repeat.selectOption('');
    await row.getByRole('button', { name: 'Cancel' }).click();
    await row.getByRole('button', { name: 'Edit task' }).click();
    await expectPriority(row, 'Low');
    await expect(repeat).toHaveValue('yearly');
  });
});

test('a resized task editor reopens at its new height (same session)', async ({
  page,
  request,
}) => {
  await withProject(request, 'e2e editor height node', async (node) => {
    const title = `e2e editor height ${Date.now()}`;
    const created = await request.post(`/api/nodes/${node.id}/tasks`, { data: { title } });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();
    await page.goto(`/nodes/${node.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Edit task"]').click();
    const editor = row.locator('textarea');
    // A drag of the native resize handle ends in a mouseup on the textarea.
    await editor.evaluate((el) => {
      (el as HTMLElement).style.height = '160px';
      el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    });
    await row.getByRole('button', { name: 'Cancel' }).click();
    // Regression: the height map was loaded once at page start, so the
    // reopened editor snapped back to the old height.
    await row.locator('button[title="Edit task"]').click();
    await expect.poll(() => editor.evaluate((el) => (el as HTMLElement).offsetHeight)).toBe(160);
  });
});

// ── One editor everywhere (TaskEditor, #132) ──────────────────────────────────
//
// My Day and the node panel render the same editor: the same
// controls, arrow keys move the priority, and Escape / Ctrl+Enter work from
// any control (they used to work only inside the title box).

test('My Day and the node panel share one task editor', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e one editor node ${stamp}`, { node_type: 'project' });
  const mk = async (url: string, data: Record<string, unknown>) => {
    const r = await request.post(url, { data: { priority: 'medium', ...data } });
    expect(r.ok()).toBeTruthy();
    return (await r.json()) as FullTask;
  };
  const surfaces = [
    { name: 'My Day', url: '/tasks/my-day', task: await mk(`/api/nodes/${node.id}/tasks`, { title: `e2e one editor myday ${stamp}`, focus_date: localISO(0) }) },
    { name: 'node panel', url: `/nodes/${node.id}`, task: await mk(`/api/nodes/${node.id}/tasks`, { title: `e2e one editor panel ${stamp}` }) },
  ];
  const priorityOnServer = async (id: string) => {
    const all: FullTask[] = await (await request.get('/api/tasks/all')).json();
    return all.find((t) => t.id === id)?.priority;
  };
  try {
    for (const s of surfaces) {
      await test.step(s.name, async () => {
        await page.goto(s.url);
        const row = page.locator(`[data-task-id="${s.task.id}"]`);
        await row.getByRole('button', { name: 'Edit task' }).click();
        const editor = row.getByTestId('task-editor');
        // The same controls in every list.
        await expect(editor.locator('textarea')).toBeVisible();
        await expect(priorityOf(editor).getByRole('radio')).toHaveText(['Low', 'Medium', 'High']);
        await expect(editor.getByTestId('due-date-field')).toBeVisible();
        await expect(editor.locator('select[title="Recurrence"]')).toBeVisible();
        await expect(editor.getByRole('button', { name: 'Save' })).toBeVisible();
        await expect(editor.getByRole('button', { name: 'Cancel' })).toBeVisible();

        // Roving tab stop: arrows move the selection and the focus.
        const medium = priorityOf(editor).getByRole('radio', { name: 'Medium' });
        await expect(medium).toHaveAttribute('tabindex', '0');
        await medium.focus();
        await page.keyboard.press('ArrowRight');
        await expectPriority(editor, 'High');
        await expect(priorityOf(editor).getByRole('radio', { name: 'High' })).toBeFocused();

        // Escape from a non-title control discards, with Undo.
        await page.keyboard.press('Escape');
        await expect(editor).toHaveCount(0);
        // …and stays on the page (the window dispatcher's Escape = "back").
        expect(new URL(page.url()).pathname).toBe(s.url);
        await page.getByRole('button', { name: 'Undo' }).click();
        await expectPriority(row.getByTestId('task-editor'), 'High');

        // Ctrl/⌘+Enter from a non-title control saves.
        await priorityOf(row.getByTestId('task-editor')).getByRole('radio', { name: 'High' }).focus();
        await page.keyboard.press('ControlOrMeta+Enter');
        await expect(row.getByTestId('task-editor')).toHaveCount(0);
        await expect.poll(() => priorityOnServer(s.task.id)).toBe('high');
      });
    }
  } finally {
    for (const s of surfaces) await request.delete(`/api/tasks/${s.task.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});
