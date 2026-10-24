import { test, expect, localISO, type APIRequestContext } from './fixtures';

/** My Day: carryover "still today?" prompt and the foldable overdue section. */

test('carried-over task prompts "still today?" — Yes re-stamps to today', async ({
  page,
  request,
}) => {
  const title = `e2e carryover ${Date.now()}`;
  const created = await request.post('/api/tasks', {
    data: { title, focus_date: localISO(-1) },
  });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();

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
    await request.delete(`/api/tasks/${task.id}`);
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
  const created = await request.post('/api/tasks', {
    data: { title, focus_date: localISO(0) },
  });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();
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
    await request.delete(`/api/tasks/${task.id}`);
  }
});

test('overdue tasks group into a foldable section', async ({ page, request }) => {
  const title = `e2e overdue ${Date.now()}`;
  const created = await request.post('/api/tasks', {
    data: { title, due_date: localISO(-3) },
  });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();

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
    await request.delete(`/api/tasks/${task.id}`);
  }
});

// ── Completion: Undo toast, carried-over "done today", recurring retraction ──
//
// Standalone (Inbox) fixtures so `/api/tasks/inbox` — every status, minus
// tombstones — can both verify server state and wipe leftovers. Wiped
// before AND after each attempt (retry-safety, .claude/rules/e2e.md).

type InboxTask = { id: string; title: string; status: string; focus_date: string | null };

const COMPLETE_PREFIX = 'e2e complete';

async function inboxTasks(request: APIRequestContext, prefix: string): Promise<InboxTask[]> {
  const resp = await request.get('/api/tasks/inbox');
  expect(resp.ok()).toBeTruthy();
  return ((await resp.json()) as InboxTask[]).filter((t) => t.title.startsWith(prefix));
}

async function wipeCompletion(request: APIRequestContext) {
  for (const t of await inboxTasks(request, COMPLETE_PREFIX)) {
    await request.delete(`/api/tasks/${t.id}`);
  }
}

test.describe('completing a task', () => {
  test.beforeEach(async ({ request }) => wipeCompletion(request));
  test.afterEach(async ({ request }) => wipeCompletion(request));

  test('carried-over task stays in Today as done; Undo restores it', async ({
    page,
    request,
  }) => {
    const title = `${COMPLETE_PREFIX} carried ${Date.now()}`;
    const created = await request.post('/api/tasks', {
      data: { title, focus_date: localISO(-2) },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();

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
    const [after] = (await inboxTasks(request, title)).filter((t) => t.id === task.id);
    expect(after).toMatchObject({ status: 'open', focus_date: localISO(-2) });
  });

  test('undoing a recurring completion leaves no duplicate occurrence', async ({
    page,
    request,
  }) => {
    const title = `${COMPLETE_PREFIX} recurring ${Date.now()}`;
    const created = await request.post('/api/tasks', {
      data: { title, focus_date: localISO(0), recurrence: 'daily' },
    });
    expect(created.ok()).toBeTruthy();
    const task = await created.json();

    await page.goto('/tasks/my-day');
    const row = page.locator('[data-task-id]', { hasText: title });
    await row.locator('button[title="Toggle done"]').click();
    const toast = page.locator('[role="status"]');
    await expect(toast.getByText('Task completed')).toBeVisible();

    // Completion scheduled tomorrow's occurrence.
    await expect.poll(async () => (await inboxTasks(request, title)).length).toBe(2);

    await toast.getByRole('button', { name: 'Undo' }).click();
    // Regression: reopening left the occurrence behind, so completing again
    // produced a duplicate. Only the original — reopened — remains.
    await expect
      .poll(async () => (await inboxTasks(request, title)).map((t) => [t.id, t.status]))
      .toEqual([[task.id, 'open']]);
  });
});

test('t on a carried-over task removes it from today (not re-stamped)', async ({
  page,
  request,
}) => {
  const title = `${COMPLETE_PREFIX} t-key ${Date.now()}`;
  const created = await request.post('/api/tasks', {
    data: { title, focus_date: localISO(-2) },
  });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();
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
        const inbox = (await (await request.get('/api/tasks/inbox')).json()) as InboxTask[];
        return inbox.find((t) => t.id === task.id)?.focus_date ?? null;
      })
      .toBeNull();
  } finally {
    await request.delete(`/api/tasks/${task.id}`);
  }
});
