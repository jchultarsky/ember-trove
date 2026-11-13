import { test, expect, createNode } from './fixtures';
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
