import { test, expect } from '@playwright/test';

/** My Day: carryover "still today?" prompt and the foldable overdue section. */

function localISO(offsetDays: number): string {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`;
}

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
    await expect(row.getByText(/carried from/)).toBeVisible();
    await expect(row.getByTestId('carryover-prompt')).toBeVisible();

    await row.getByRole('button', { name: 'Yes', exact: true }).click();
    await expect(page.locator('[role="status"]').getByText('Kept for today')).toBeVisible();
    // After the refetch the focus date is today → badge and prompt are gone.
    const fresh = page.locator('[data-task-id]', { hasText: title });
    await expect(fresh).toBeVisible();
    await expect(fresh.getByText(/carried from/)).not.toBeVisible();
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
