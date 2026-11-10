import { test, expect, createNode, createNote, localISO } from './fixtures';

/**
 * Phone-width layout (#262): the pass the issue asked for, kept as a test.
 * At 375px no page may scroll sideways, and a long title must get the
 * width of the screen, not a few characters a line beside its icons.
 */

test.use({ viewport: { width: 375, height: 812 } });

const LONG = 'cabinets, counters and the long-delayed lighting plan';

test('no page scrolls sideways at phone width', async ({ page, request }) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e mobile ${stamp}: ${LONG}`, {
    node_type: 'project',
    body: '## Goals\n\n- One\n- Two\n\nSome ordinary prose that wraps.',
  });
  try {
    await createNote(request, project.id, 'A note with a [link](https://example.com/a/long/path).');
    const task = await request.post(`/api/nodes/${project.id}/tasks`, {
      data: {
        title: 'Call the electrician about the under-cabinet lighting quote',
        due_date: localISO(-2),
        focus_date: localISO(0),
        priority: 'high',
      },
    });
    expect(task.ok()).toBeTruthy();

    for (const path of [
      '/tasks/my-day',
      '/tasks/captures',
      '/tasks/calendar',
      '/dashboard',
      '/nodes',
      `/nodes/${project.id}`,
      `/nodes/${project.id}/edit`,
      '/notes',
      '/people',
      '/search?q=mobile',
      '/tags',
      '/templates',
      '/webhooks',
      '/data-health',
    ]) {
      await page.goto(path);
      const main = page.locator('main');
      await expect(main).toBeVisible();
      // Let the page's data arrive before measuring.
      await page.waitForLoadState('networkidle');
      const over = await main.evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(over, `${path} is ${over}px wider than the screen`).toBeLessThanOrEqual(1);
    }
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a long node title gets the width of the phone', async ({ page, request }) => {
  const stamp = Date.now();
  const title = `e2e mobile title ${stamp}: ${LONG}`;
  const project = await createNode(request, title, { node_type: 'project' });
  try {
    await page.goto(`/nodes/${project.id}`);
    const heading = page.getByRole('heading', { name: title });
    await expect(heading).toBeVisible();
    const box = await heading.boundingBox();
    // It was about 120px wide and eight lines tall, beside its icons.
    expect(box!.width).toBeGreaterThan(250);
    expect(box!.height).toBeLessThan(120);
    // The project status chip is on the screen, not past its edge.
    const chip = await page.getByRole('button', { name: /Active/ }).first().boundingBox();
    expect(chip!.x + chip!.width).toBeLessThanOrEqual(375);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('the calendar header keeps its subtitle on one line', async ({ page }) => {
  await page.goto('/tasks/calendar');
  const subtitle = page.getByText('Tasks by due date');
  await expect(subtitle).toBeVisible();
  // One line of text-xs is about 16px; it wrapped to four beside the
  // month navigation.
  expect((await subtitle.boundingBox())!.height).toBeLessThan(24);
});
