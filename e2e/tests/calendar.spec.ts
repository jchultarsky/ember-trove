import { test, expect, createNode } from './fixtures';

/**
 * Calendar quick-add (#234): clicking a day opens a composer. With a
 * Project it makes that Project's task due that day; without one, a capture
 * led by the date (a task belongs to a Project, and a capture has no date).
 */

const todayISO = () => {
  // Local date (the spec's Node and the browser share the container TZ).
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(
    d.getDate(),
  ).padStart(2, '0')}`;
};

test('a day with a Project adds that Project a task due that day', async ({ page, request }) => {
  const stamp = Date.now();
  const title = `e2e calendar ${stamp}`;
  const project = await createNode(request, `e2e calendar project ${stamp}`, { node_type: 'project' });
  try {
    await page.goto('/tasks/calendar');
    const cell = page.locator(`[data-date="${todayISO()}"]`);
    await cell.click();
    const composer = cell.getByTestId('calendar-composer');
    const picker = composer.getByRole('combobox', { name: 'Project (optional)…' });
    await picker.fill(`e2e calendar project ${stamp}`);
    await page.getByRole('option', { name: new RegExp(`calendar project ${stamp}`) }).click();
    const input = composer.getByPlaceholder('Task or capture…');
    await input.fill(title);
    await input.press('Enter');

    await expect(page.locator('[role="status"]').getByText(/Task added to/)).toBeVisible();
    await expect(cell.getByText(title)).toBeVisible();
    const tasks: { title: string; due_date: string }[] = await (
      await request.get(`/api/nodes/${project.id}/tasks`)
    ).json();
    expect(tasks.find((t) => t.title === title)?.due_date).toBe(todayISO());
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a day without a Project makes a capture led by the date', async ({ page, request }) => {
  const title = `e2e calendar capture ${Date.now()}`;
  await page.goto('/tasks/calendar');
  const cell = page.locator(`[data-date="${todayISO()}"]`);
  await cell.click();
  const input = cell.getByPlaceholder('Task or capture…');
  await input.fill(title);
  await input.press('Enter');

  await expect(page.locator('[role="status"]').getByText(/Added to Captures, dated/)).toBeVisible();
  const captures: { id: string; body: string }[] = await (await request.get('/api/captures')).json();
  const made = captures.find((c) => c.body.endsWith(`: ${title}`));
  expect(made?.body).toMatch(/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) \d{1,2} [A-Z][a-z]{2}: /);
  await request.delete(`/api/captures/${made!.id}`);
});

test('Escape closes the composer without creating anything', async ({ page, request }) => {
  await page.goto('/tasks/calendar');
  const cell = page.locator(`[data-date="${todayISO()}"]`);
  await cell.click();
  const input = cell.getByPlaceholder('Task or capture…');
  await expect(input).toBeVisible();
  await input.fill('never created');
  await input.press('Escape');
  await expect(input).not.toBeVisible();

  const inbox = await (await request.get('/api/tasks/inbox')).json();
  expect(inbox.some((t: { title: string }) => t.title === 'never created')).toBe(false);
  const captures = await (await request.get('/api/captures')).json();
  expect(captures.some((c: { body: string }) => c.body.includes('never created'))).toBe(false);
});
