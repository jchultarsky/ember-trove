import { test, expect, createNode, localISO } from './fixtures';
import type { APIRequestContext } from '@playwright/test';

/**
 * Task states (S5a, #314; ONTOLOGY §9.3): a task is Open, Waiting, Done or
 * Cancelled, and Done carries its completion date.
 */

async function addTask(request: APIRequestContext, nodeId: string, data: Record<string, unknown>) {
  const resp = await request.post(`/api/nodes/${nodeId}/tasks`, { data });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

async function setStatus(request: APIRequestContext, taskId: string, status: string) {
  const resp = await request.patch(`/api/tasks/${taskId}`, { data: { status } });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

test('a task has its completion date exactly while it is done', async ({ request }) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states date ${stamp}`, { node_type: 'project' });
  try {
    const task = await addTask(request, project.id, { title: `report ${stamp}` });
    expect(task.status).toBe('open');
    expect(task.completed_at).toBeNull();

    const done = await setStatus(request, task.id, 'done');
    expect(done.completed_at).toBeTruthy();
    // A rename is not a completion: the date stands.
    const renamed = await request.patch(`/api/tasks/${task.id}`, {
      data: { title: `report, v2 ${stamp}` },
    });
    expect((await renamed.json()).completed_at).toBe(done.completed_at);

    for (const state of ['open', 'waiting', 'cancelled']) {
      await setStatus(request, task.id, 'done');
      const left = await setStatus(request, task.id, state);
      expect(left.status).toBe(state);
      expect(left.completed_at, state).toBeNull();
    }

    // The word a client from before S5a sends is read as open, never stored.
    const old = await addTask(request, project.id, {
      title: `old client ${stamp}`,
      status: 'in_progress',
    });
    expect(old.status).toBe('open');
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('the dashboard counts each state and lists what can be acted on first', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const title = `e2e states card ${stamp}`;
  const project = await createNode(request, title, { node_type: 'project' });
  try {
    // The waiting task is the most urgent by priority, and still comes last.
    await addTask(request, project.id, {
      title: `ask the vendor ${stamp}`,
      status: 'waiting',
      priority: 'high',
    });
    await addTask(request, project.id, { title: `write it up ${stamp}`, priority: 'low' });
    await addTask(request, project.id, { title: `book the room ${stamp}`, priority: 'medium' });
    const finished = await addTask(request, project.id, { title: `agree the scope ${stamp}` });
    await setStatus(request, finished.id, 'done');
    const dropped = await addTask(request, project.id, { title: `print handouts ${stamp}` });
    await setStatus(request, dropped.id, 'cancelled');

    await page.goto('/dashboard');
    const card = page
      .getByTestId('project-groups')
      .getByTestId('project-card')
      .filter({ hasText: title });
    await expect(card.getByTestId('count-open')).toHaveText('2');
    await expect(card.getByTestId('count-waiting')).toHaveText('1');
    await expect(card.getByTestId('count-done')).toHaveText('1');
    await expect(card.getByTestId('count-cancelled')).toHaveText('1');
    // Four bare numbers: each says which it is.
    await expect(card.getByTestId('count-waiting')).toHaveAccessibleName('1 waiting');
    await expect(card.getByTestId('count-open')).toHaveAttribute('title', '2 open');

    // "To do" holds the open tasks and the waiting one: three, not two.
    const tasks = card.getByTestId('card-tasks');
    await expect(tasks).toContainText('To do (3)');
    await expect(tasks.getByTestId('task-chip')).toContainText([
      `book the room ${stamp}`,
      `write it up ${stamp}`,
      `ask the vendor ${stamp}`,
    ]);
    await expect(tasks.getByTestId('task-chip-waiting')).toHaveCount(1);
    await expect(
      tasks.getByTestId('task-chip').last().getByTestId('task-chip-waiting'),
    ).toHaveText('Waiting');
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a task is set to Waiting and back from its editor', async ({ page, request }) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states editor ${stamp}`, {
    node_type: 'project',
  });
  try {
    const task = await addTask(request, project.id, { title: `ask the vendor ${stamp}` });
    await page.goto(`/nodes/${project.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await expect(row).toBeVisible();
    await expect(row.getByTestId('task-state')).toHaveCount(0);

    // The picker sits in the one task editor: Open, Waiting, Cancelled.
    await row.getByRole('button', { name: 'Edit task' }).click();
    const picker = row.getByTestId('task-editor').getByTestId('state-picker');
    await expect(picker.getByRole('radio')).toHaveText(['Open', 'Waiting', 'Cancelled']);
    await expect(picker.getByRole('radio', { name: 'Open' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    await picker.getByRole('radio', { name: 'Waiting' }).click();
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(row.getByTestId('task-state')).toHaveText('Waiting');
    const stored = await (await request.get(`/api/nodes/${project.id}/tasks`)).json();
    expect(stored[0].status).toBe('waiting');

    // Saving a new title alone leaves the state as it is.
    await row.getByRole('button', { name: 'Edit task' }).click();
    await row.getByTestId('task-editor').locator('textarea').fill(`ask the vendor again ${stamp}`);
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(row).toContainText(`ask the vendor again ${stamp}`);
    await expect(row.getByTestId('task-state')).toHaveText('Waiting');

    // Arrow keys move the choice, as in the priority picker.
    await row.getByRole('button', { name: 'Edit task' }).click();
    const waiting = picker.getByRole('radio', { name: 'Waiting' });
    await expect(waiting).toHaveAttribute('aria-checked', 'true');
    await waiting.focus();
    await waiting.press('ArrowLeft');
    await expect(picker.getByRole('radio', { name: 'Open' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    await expect(picker.getByRole('radio', { name: 'Open' })).toBeFocused();
    await picker.getByRole('radio', { name: 'Open' }).press('ControlOrMeta+Enter');
    await expect(row.getByTestId('task-editor')).toHaveCount(0);
    await expect(row.getByTestId('task-state')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a done task shows when it was completed, and a cancelled one says so', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states done ${stamp}`, { node_type: 'project' });
  try {
    const finished = await addTask(request, project.id, { title: `agree the scope ${stamp}` });
    const dropped = await addTask(request, project.id, { title: `print handouts ${stamp}` });
    await page.goto(`/nodes/${project.id}`);

    // Cancelled from the editor.
    const droppedRow = page.locator(`[data-task-id="${dropped.id}"]`);
    await droppedRow.getByRole('button', { name: 'Edit task' }).click();
    await droppedRow
      .getByTestId('task-editor')
      .getByTestId('state-picker')
      .getByRole('radio', { name: 'Cancelled' })
      .click();
    await droppedRow.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    // Done with the checkbox.
    const finishedRow = page.locator(`[data-task-id="${finished.id}"]`);
    await finishedRow.locator('button[title="Toggle done"]').click();

    // Both are among the finished tasks now.
    await page.getByRole('button', { name: /2 completed/ }).click();
    await expect(droppedRow.getByTestId('task-state')).toHaveText('Cancelled');
    await expect(droppedRow.getByTestId('task-completed')).toHaveCount(0);
    await expect(finishedRow.getByTestId('task-completed')).toHaveText(/^Done [A-Z][a-z]{2} \d{1,2}$/);
    await expect(finishedRow.getByTestId('task-state')).toHaveCount(0);

    // A done task is reopened with its checkbox: its editor has no picker.
    await finishedRow.getByRole('button', { name: 'Edit task' }).click();
    await expect(finishedRow.getByTestId('task-editor')).toBeVisible();
    await expect(finishedRow.getByTestId('state-picker')).toHaveCount(0);
    // A cancelled one can be put back.
    await droppedRow.getByRole('button', { name: 'Edit task' }).click();
    await expect(
      droppedRow.getByTestId('state-picker').getByRole('radio', { name: 'Cancelled' }),
    ).toHaveAttribute('aria-checked', 'true');
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a Waiting task is out of Today and Overdue, and stays in the Backlog', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states my day ${stamp}`, {
    node_type: 'project',
  });
  try {
    const carried = await addTask(request, project.id, {
      title: `carried ${stamp}`,
      focus_date: localISO(-1),
    });
    // Focused yesterday and past its due date: both would put it in front
    // of its person. It is waiting on someone else.
    const waiting = await addTask(request, project.id, {
      title: `on the vendor ${stamp}`,
      status: 'waiting',
      focus_date: localISO(-1),
      due_date: localISO(-3),
    });
    const late = await addTask(request, project.id, {
      title: `late ${stamp}`,
      due_date: localISO(-3),
    });

    await page.goto('/tasks/my-day');
    const today = page.getByTestId('zone-today');
    const overdue = page.getByTestId('overdue-section');
    const backlog = page.getByTestId('zone-backlog');
    const rowIn = (zone: typeof today, id: string) => zone.locator(`[data-task-id="${id}"]`);

    await expect(rowIn(today, carried.id)).toBeVisible();
    await expect(rowIn(overdue, late.id)).toBeVisible();
    await expect(rowIn(today, waiting.id)).toHaveCount(0);
    await expect(rowIn(overdue, waiting.id)).toHaveCount(0);

    // Not hidden: in the backlog, marked, with its date, and no "keep on
    // today?" question.
    const row = rowIn(backlog, waiting.id);
    await expect(row.getByTestId('task-state')).toHaveText('Waiting');
    await expect(row.getByText(/due [A-Z][a-z]{2} \d/)).toBeVisible();
    await expect(row.getByTestId('carryover-prompt')).toHaveCount(0);

    // The vendor answers: open again, and it is back in the normal flow.
    await setStatus(request, waiting.id, 'open');
    await page.reload();
    await expect(rowIn(today, waiting.id)).toBeVisible();
    await expect(rowIn(today, waiting.id).getByTestId('carryover-prompt')).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a state whose save failed is sent again on the retry', async ({ page, request }) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states retry ${stamp}`, { node_type: 'project' });
  try {
    const task = await addTask(request, project.id, { title: `ask the vendor ${stamp}` });
    await page.goto(`/nodes/${project.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);
    const picker = row.getByTestId('state-picker');

    let fail = true;
    await page.route(`**/api/tasks/${task.id}`, (route) =>
      fail && route.request().method() === 'PATCH'
        ? route.fulfill({ status: 500, body: 'no' })
        : route.continue(),
    );
    await row.getByRole('button', { name: 'Edit task' }).click();
    await picker.getByRole('radio', { name: 'Waiting' }).click();
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText(/Save failed/)).toBeVisible();
    await expect(row.getByTestId('task-state')).toHaveCount(0);

    // The editor still holds the choice, and the retry sends it. It used
    // to count the state as saved once it had been sent, and leave it out.
    fail = false;
    await row.getByRole('button', { name: 'Edit task' }).click();
    await expect(picker.getByRole('radio', { name: 'Waiting' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(row.getByTestId('task-state')).toHaveText('Waiting');
    const stored = await (await request.get(`/api/nodes/${project.id}/tasks`)).json();
    expect(stored[0].status).toBe('waiting');
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test("the editor's picker follows a task ticked or unticked outside it", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states follow ${stamp}`, { node_type: 'project' });
  try {
    const task = await addTask(request, project.id, { title: `write it up ${stamp}` });
    await page.goto(`/nodes/${project.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);

    // An edit and a save: the list keeps this task's editor state from here.
    await row.getByRole('button', { name: 'Edit task' }).click();
    await row.getByTestId('task-editor').locator('textarea').fill(`write it up, v2 ${stamp}`);
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(row).toContainText(`write it up, v2 ${stamp}`);

    // Ticked with the checkbox: its editor has no state picker now.
    await row.locator('button[title="Toggle done"]').click();
    await page.getByRole('button', { name: /1 completed/ }).click();
    await row.getByRole('button', { name: 'Edit task' }).click();
    await expect(row.getByTestId('task-editor')).toBeVisible();
    await expect(row.getByTestId('state-picker')).toHaveCount(0);
    await row.getByTestId('task-editor').getByRole('button', { name: 'Cancel' }).click();

    // Unticked: the picker is back, on Open.
    await row.locator('button[title="Toggle done"]').click();
    await expect(row.locator('button[title="Toggle done"]')).toBeVisible();
    await row.getByRole('button', { name: 'Edit task' }).click();
    await expect(
      row.getByTestId('state-picker').getByRole('radio', { name: 'Open' }),
    ).toHaveAttribute('aria-checked', 'true');
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a state saved in the editor does not come back after the task is ticked', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = await createNode(request, `e2e states stale ${stamp}`, { node_type: 'project' });
  try {
    const task = await addTask(request, project.id, { title: `chase the quote ${stamp}` });
    await page.goto(`/nodes/${project.id}`);
    const row = page.locator(`[data-task-id="${task.id}"]`);

    // Waiting, saved from the editor.
    await row.getByRole('button', { name: 'Edit task' }).click();
    await row.getByTestId('state-picker').getByRole('radio', { name: 'Waiting' }).click();
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(row.getByTestId('task-state')).toHaveText('Waiting');

    // The quote arrives: done, with the checkbox.
    await row.locator('button[title="Toggle done"]').click();
    await page.getByRole('button', { name: /1 completed/ }).click();
    await expect(row.getByTestId('task-completed')).toBeVisible();

    // A title edit on the done task must send no state. The editor used to
    // hold on to "waiting" and send it: the task came back un-completed.
    await row.getByRole('button', { name: 'Edit task' }).click();
    await expect(row.getByTestId('state-picker')).toHaveCount(0);
    await row.getByTestId('task-editor').locator('textarea').fill(`quote received ${stamp}`);
    await row.getByTestId('task-editor').getByRole('button', { name: 'Save' }).click();
    await expect(row).toContainText(`quote received ${stamp}`);
    const stored = (await (await request.get(`/api/nodes/${project.id}/tasks`)).json())[0];
    expect(stored.status).toBe('done');
    expect(stored.completed_at).toBeTruthy();
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the state picker fits the task editor', async ({ page, request }) => {
    const stamp = Date.now();
    const project = await createNode(request, `e2e states phone ${stamp}`, {
      node_type: 'project',
    });
    try {
      const task = await addTask(request, project.id, { title: `narrow ${stamp}` });
      await page.goto(`/nodes/${project.id}`);
      const row = page.locator(`[data-task-id="${task.id}"]`);
      await row.getByRole('button', { name: 'Edit task' }).click();
      const editor = row.getByTestId('task-editor');
      // Neither picker clips its last option ("Cancelled" was cut by 8px).
      for (const id of ['priority-picker', 'state-picker']) {
        const picker = editor.getByTestId(id);
        await expect(picker).toBeVisible();
        const clipped = await picker.evaluate((el) => el.scrollWidth - el.clientWidth);
        expect(clipped, `${id} is ${clipped}px wider than its box`).toBeLessThanOrEqual(0);
        // On one line: a picker too wide for its box wraps now, it doesn't
        // clip, so "not clipped" alone would pass with it in two rows.
        const tops = await picker
          .getByRole('radio')
          .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().top)));
        expect(new Set(tops).size, `${id} wrapped`).toBe(1);
      }
      // And the page still doesn't scroll sideways with the editor open.
      const over = await page.locator('main').evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(over).toBeLessThanOrEqual(1);
    } finally {
      await request.delete(`/api/nodes/${project.id}`);
    }
  });

  test("the pickers fit My Day's task editor", async ({ page, request }) => {
    const stamp = Date.now();
    const project = await createNode(request, `e2e states phone day ${stamp}`, {
      node_type: 'project',
    });
    try {
      const task = await addTask(request, project.id, {
        title: `narrower still ${stamp}`,
        focus_date: localISO(0),
      });
      await page.goto('/tasks/my-day');
      const row = page.locator(`[data-task-id="${task.id}"]`);
      await row.locator('button[title="Edit task"]').click();
      const editor = row.getByTestId('task-editor');
      await expect(editor).toBeVisible();
      // My Day's row is narrower than the Project page's: beside its action
      // buttons the editor was about 150px wide, less than the state picker.
      for (const id of ['priority-picker', 'state-picker']) {
        const picker = editor.getByTestId(id);
        const clipped = await picker.evaluate((el) => el.scrollWidth - el.clientWidth);
        expect(clipped, `${id} is ${clipped}px wider than its box`).toBeLessThanOrEqual(0);
        // On one line: nothing wrapped under the first option.
        const tops = await picker
          .getByRole('radio')
          .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().top)));
        expect(new Set(tops).size, `${id} wrapped`).toBe(1);
      }
      const over = await page.locator('main').evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(over).toBeLessThanOrEqual(1);
    } finally {
      await request.delete(`/api/nodes/${project.id}`);
    }
  });
});
