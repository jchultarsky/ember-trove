import { test, expect } from '@playwright/test';

/** Calendar quick-add: clicking a day cell composes a task due that day. */

test('clicking a day adds a task due that day', async ({ page, request }) => {
  const title = `e2e calendar ${Date.now()}`;
  // Local date (the spec's Node and the browser share the container TZ).
  const d = new Date();
  const todayISO = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`;

  await page.goto('/tasks/calendar');
  const cell = page.locator(`[data-date="${todayISO}"]`);
  await expect(cell).toBeVisible();

  await cell.click();
  const input = cell.getByPlaceholder('New task…');
  await expect(input).toBeVisible();
  await input.fill(title);
  await input.press('Enter');

  await expect(page.locator('[role="status"]').getByText(/Task added/)).toBeVisible();
  // The chip renders in the same cell after the refetch.
  await expect(cell.getByText(title)).toBeVisible();

  // Server state: standalone task with the clicked due date; then clean up.
  const inbox = await (await request.get('/api/tasks/inbox')).json();
  const created = inbox.find((t: { title: string }) => t.title === title);
  expect(created?.due_date).toBe(todayISO);
  await request.delete(`/api/tasks/${created.id}`);
});

test('Escape closes the composer without creating anything', async ({ page, request }) => {
  const d = new Date();
  const todayISO = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`;

  await page.goto('/tasks/calendar');
  const cell = page.locator(`[data-date="${todayISO}"]`);
  await cell.click();
  const input = cell.getByPlaceholder('New task…');
  await expect(input).toBeVisible();
  await input.fill('never created');
  await input.press('Escape');
  await expect(input).not.toBeVisible();

  const inbox = await (await request.get('/api/tasks/inbox')).json();
  expect(inbox.some((t: { title: string }) => t.title === 'never created')).toBe(false);
});
