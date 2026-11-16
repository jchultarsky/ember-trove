import {
  test,
  expect,
  localISO,
  createProject,
  addTask,
  tasksOf,
  deleteNodesByPrefix,
  type APIRequestContext,
} from './fixtures';

/** My Day: carryover "still today?" prompt and the foldable overdue section. */

// A task needs its Project (S5c, ONTOLOGY §9.3), so each test makes one for
// its tasks. Deleting the Project takes them with it; the wipe removes what
// a crashed attempt left (retry-safety, .claude/rules/e2e.md). The title
// holds no word a test looks for in a row ("carried", "done", "Overdue").
const HOST_PREFIX = 'e2e myday host ';

const host = (request: APIRequestContext) =>
  createProject(request, `${HOST_PREFIX}${Date.now()}`);

test.beforeEach(async ({ request }) => deleteNodesByPrefix(request, HOST_PREFIX));
test.afterEach(async ({ request }) => deleteNodesByPrefix(request, HOST_PREFIX));

test('carried-over task prompts "still today?" — Yes re-stamps to today', async ({
  page,
  request,
}) => {
  const title = `e2e carryover ${Date.now()}`;
  const project = await host(request);
  await addTask(request, project.id, { title, focus_date: localISO(-1) });

  try {
    await page.goto('/tasks/my-day');
    const row = page.locator('[data-task-id]', { hasText: title });
    await expect(row).toBeVisible();
    await expect(row.getByText(/carried/)).toBeVisible();
    await expect(row.getByTestId('carryover-prompt')).toBeVisible();

    await row.getByRole('button', { name: 'Keep on today' }).click();
    await expect(page.locator('[role="status"]').getByText('Kept for today')).toBeVisible();
    // After the refetch the focus date is today → badge and prompt are gone.
    const fresh = page.locator('[data-task-id]', { hasText: title });
    await expect(fresh).toBeVisible();
    await expect(fresh.getByText(/carried/)).not.toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('long task titles wrap on phone widths, truncate on desktop', async ({
  page,
  request,
}) => {
  // On an iPhone the My Day column is narrow and `truncate` cut titles to
  // one ellipsised line. Small screens now wrap the full title; sm+ keeps
  // the single-line truncation for list density.
  const stamp = Date.now();
  const title =
    `e2e wrap ${stamp} — a deliberately verbose task title that cannot ` +
    `possibly fit on a single 375px-wide line of small text`;
  const project = await host(request);
  await addTask(request, project.id, { title, focus_date: localISO(0) });
  const SINGLE_LINE_MAX_PX = 30; // text-sm line ≈ 20px; 2 wrapped lines ≥ 36px

  try {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto('/tasks/my-day');
    const titleEl = page.getByText(`e2e wrap ${stamp}`, { exact: false });
    await expect(titleEl).toBeVisible();
    const mobile = await titleEl.boundingBox();
    expect(mobile!.height).toBeGreaterThan(SINGLE_LINE_MAX_PX);

    await page.setViewportSize({ width: 1280, height: 720 });
    const desktop = await titleEl.boundingBox();
    expect(desktop!.height).toBeLessThan(SINGLE_LINE_MAX_PX);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('overdue tasks group into a foldable section', async ({ page, request }) => {
  const title = `e2e overdue ${Date.now()}`;
  const project = await host(request);
  await addTask(request, project.id, { title, due_date: localISO(-3) });

  try {
    await page.goto('/tasks/my-day');
    const section = page.getByTestId('overdue-section');
    await expect(section.getByText(/Overdue · \d+/)).toBeVisible();
    const row = section.locator('[data-task-id]', { hasText: title });
    await expect(row).toBeVisible();

    // Folding hides the rows but keeps the header with the count.
    await section.getByText(/Overdue · \d+/).click();
    await expect(row).not.toBeVisible();
    await expect(section.getByText(/Overdue · \d+/)).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

// ── Completion: Undo toast, carried-over "done today", one task stays one ──
//
// The server's state is read from the task's Project: every task on it,
// whatever its status, minus tombstones (`tasksOf`).

const COMPLETE_PREFIX = 'e2e complete';

test.describe('completing a task', () => {

  test('carried-over task stays in Today as done; Undo restores it', async ({
    page,
    request,
  }) => {
    const title = `${COMPLETE_PREFIX} carried ${Date.now()}`;
    const project = await host(request);
    const task = await addTask(request, project.id, { title, focus_date: localISO(-2) });

    await page.goto('/tasks/my-day');
    const row = page.locator('[data-task-id]', { hasText: title });
    await expect(row).toBeVisible();
    const counter = page.getByText(/^\d+ \/ \d+ done$/);
    await expect(counter).toBeVisible();
    const [done0, total0] = ((await counter.textContent()) ?? '').match(/\d+/g)!.map(Number);

    await row.locator('button[title="Toggle done"]').click();
    const toast = page.locator('[role="status"]');
    await expect(toast.getByText('Task completed')).toBeVisible();

    // Regression: the completed carry-over used to drop out of My Day,
    // shrinking the denominator ("0 / 3 → 0 / 2") instead of counting.
    await expect(counter).toHaveText(`${done0 + 1} / ${total0} done`);
    await expect(row.locator('button[title="Toggle done"]')).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    await toast.getByRole('button', { name: 'Undo' }).click();
    await expect(counter).toHaveText(`${done0} / ${total0} done`);
    await expect(row.locator('button[title="Toggle done"]')).toHaveAttribute(
      'aria-pressed',
      'false',
    );
    // Undo restores the original commitment date, not just the status.
    await expect(row.getByTestId('carryover-prompt')).toBeVisible();
    const [after] = (await tasksOf(request, project.id)).filter((t) => t.id === task.id);
    expect(after).toMatchObject({ status: 'open', focus_date: localISO(-2) });
  });

  test('completing a task makes no second one, and Undo reopens that one', async ({
    page,
    request,
  }) => {
    // S5b (ONTOLOGY §9): tasks no longer repeat. The fixture is what a tab
    // left open across that release still sends, a repeat rule: it is read
    // past, not refused, and the answer names none.
    const title = `${COMPLETE_PREFIX} once ${Date.now()}`;
    const project = await host(request);
    const task = await addTask(request, project.id, {
      title,
      focus_date: localISO(0),
      recurrence: 'daily',
    });
    expect(task).not.toHaveProperty('recurrence');

    // The save's own answer, not a toast: whatever a completion does on the
    // server, it has done by the time the PATCH answers.
    const saved = () =>
      page.waitForResponse(
        (r) => r.request().method() === 'PATCH' && r.url().endsWith(`/api/tasks/${task.id}`),
      );
    const onServer = async () =>
      (await tasksOf(request, project.id)).map((t) => [t.id, t.status]);

    await page.goto('/tasks/my-day');
    const row = page.locator('[data-task-id]', { hasText: title });
    const completed = saved();
    await row.locator('button[title="Toggle done"]').click();
    expect((await completed).ok()).toBeTruthy();
    // Completing used to schedule tomorrow's occurrence: two tasks.
    expect(await onServer()).toEqual([[task.id, 'done']]);

    const toast = page.locator('[role="status"]');
    await expect(toast.getByText('Task completed')).toBeVisible();
    const reopened = saved();
    await toast.getByRole('button', { name: 'Undo' }).click();
    expect((await reopened).ok()).toBeTruthy();
    expect(await onServer()).toEqual([[task.id, 'open']]);
  });
});

test('t on a carried-over task removes it from today (not re-stamped)', async ({
  page,
  request,
}) => {
  const title = `${COMPLETE_PREFIX} t-key ${Date.now()}`;
  const project = await host(request);
  const task = await addTask(request, project.id, { title, focus_date: localISO(-2) });
  try {
    await page.goto('/tasks/my-day');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await expect(row).toBeVisible();
    // Neutral focus for single-key shortcuts (e2e.md), then walk the j/k
    // cursor to the row (its inline style gains the inset focus ring).
    await page.getByRole('heading', { name: 'My Day' }).click();
    let focused = false;
    for (let i = 0; i < 40 && !focused; i++) {
      await page.keyboard.press('j');
      focused = await row.evaluate((el) => (el as HTMLElement).style.boxShadow.includes('inset'));
    }
    expect(focused).toBe(true);
    await page.keyboard.press('t');
    // Regression: `t` re-stamped a carried-over task to today instead of
    // removing it (the Today zone is focus_date <= today).
    await expect
      .poll(async () => {
        const [now] = (await tasksOf(request, project.id)).filter((t) => t.id === task.id);
        // `undefined` while the task isn't listed: only a cleared date counts.
        return now ? now.focus_date : undefined;
      })
      .toBeNull();
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});
