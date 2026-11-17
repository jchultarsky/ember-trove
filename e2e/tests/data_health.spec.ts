import {
  test,
  expect,
  createNode,
  createProject,
  addTask,
  tasksOf,
  deleteNodesByPrefix,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * The Data health page (#208): the caller's data that doesn't fit the data
 * model, per rule, read-only, each item linking to where it lives. Counts
 * vary with whatever else the shared DB holds, so everything is asserted by
 * id, never by count.
 */

type Report = { rules: { rule: string; count: number; items: { subject: { id: string } }[] }[] };

const subjectsOf = async (request: APIRequestContext, rule: string): Promise<string[]> => {
  const report: Report = await (await request.get('/api/data-health')).json();
  return report.rules.find((r) => r.rule === rule)?.items.map((i) => i.subject.id) ?? [];
};

/** Retry-safe (`.claude/rules/e2e.md`): remove anything a crashed earlier
 * attempt left behind before starting, and everything after. */
const PREFIX = 'e2e dh ';

const wipe = (request: APIRequestContext) => deleteNodesByPrefix(request, PREFIX);

/**
 * Serve the page a "Tasks outside a Project" card that lists the real tasks
 * on `home`. The API refuses a task on anything but a Project (#216) and a
 * task with no Project at all (S5c), so no call can make the data this card
 * is for: only older data has it. `home` is a real Project standing in for
 * that older node, and the items are read from it on every load, so the
 * card's fixes run against the real API and the card shows what they did.
 */
async function serveTasksOutsideAProject(
  page: Page,
  request: APIRequestContext,
  home: { id: string; title: string },
) {
  await page.route('**/api/data-health', async (route) => {
    const items = (await tasksOf(request, home.id)).map((t) => ({
      subject: {
        kind: 'task',
        id: t.id,
        label: t.title,
        node_id: home.id,
        closed: t.status === 'done' || t.status === 'cancelled',
      },
      related: [{ kind: 'node', id: home.id, label: home.title }],
      more: 0,
    }));
    await route.fulfill({
      json: { rules: [{ rule: 'task_on_non_project', count: items.length, items }] },
    });
  });
}

const idsOn = async (request: APIRequestContext, nodeId: string): Promise<string[]> =>
  (await tasksOf(request, nodeId)).map((t) => t.id);

test('what the model forbids is refused, and has no card', async ({ page, request }) => {
  await wipe(request);
  try {
    const stamp = Date.now();
    const area = await createNode(request, `${PREFIX}area ${stamp}`, { node_type: 'area' });
    const a = await createProject(request, `${PREFIX}pair a ${stamp}`);
    const b = await createProject(request, `${PREFIX}pair b ${stamp}`);
    // A task on an Area is refused (§3.2, #216): it can no longer be made
    // through the API, so that rule is covered by the pg tests, and here
    // the refusal itself is checked.
    const onArea = await request.post(`/api/nodes/${area.id}/tasks`, {
      data: { title: `${PREFIX}task on area ${stamp}` },
    });
    expect(onArea.status()).toBe(422);
    expect((await onArea.json()).error).toBe('only Projects have tasks');
    // A task with no Project at all is refused too (§9.3, S5c), whether
    // it is made that way or taken off its Project.
    const loose = await request.post('/api/tasks', {
      data: { title: `${PREFIX}loose ${stamp}` },
    });
    expect(loose.status()).toBe(422);
    expect((await loose.json()).error).toBe('a task needs a Project');
    const task = await addTask(request, a.id, { title: `${PREFIX}stays ${stamp}` });
    const detached = await request.patch(`/api/tasks/${task.id}`, { data: { node_id: null } });
    expect(detached.status()).toBe(422);
    expect((await detached.json()).error).toContain('a task needs a Project');
    expect(await idsOn(request, a.id)).toEqual([task.id]);
    // Moving it to another Project is still an edit like any other.
    const moved = await request.patch(`/api/tasks/${task.id}`, { data: { node_id: b.id } });
    expect(moved.ok()).toBeTruthy();
    expect(await idsOn(request, b.id)).toEqual([task.id]);
    // related_to is stored once (#226): the reverse is the same link, so
    // that rule can no longer be broken through the API. The pg tests cover
    // the rule on older data; here the refusal itself is checked.
    const ab = await request.post('/api/edges', {
      data: { source_id: a.id, target_id: b.id, edge_type: 'related_to' },
    });
    expect(ab.ok()).toBeTruthy();
    const ba = await request.post('/api/edges', {
      data: { source_id: b.id, target_id: a.id, edge_type: 'related_to' },
    });
    expect(ba.status()).toBe(409);
    expect((await ba.json()).error).toBe('already linked as related to');

    // "Tasks with no Project" is no rule any more: not in the report, and
    // no card on the page.
    const report: Report = await (await request.get('/api/data-health')).json();
    const rules = report.rules.map((r) => r.rule);
    expect(rules).toContain('task_on_non_project');
    expect(rules).not.toContain('orphan_task');
    expect(await subjectsOf(request, 'task_on_non_project')).not.toContain(task.id);
    // Looked at only once the page has its report: before that there is
    // no card of any kind.
    const loaded = page.waitForResponse(
      (r) => r.url().endsWith('/api/data-health') && r.request().method() === 'GET',
    );
    await page.goto('/data-health');
    expect((await loaded).ok()).toBeTruthy();
    await expect(page.getByRole('heading', { name: 'Data health' })).toBeVisible();
    await expect(page.getByTestId('data-health-task_on_non_project')).toHaveCount(0);
    await expect(page.getByTestId('data-health-orphan_task')).toHaveCount(0);
  } finally {
    await wipe(request);
  }
});

test('a task outside a Project links into its node', async ({ page, request }) => {
  // Since #216 the API refuses a task on a non-Project, so this card can only
  // come from older data. Serve such a report to cover the card and its link.
  const node = await createNode(request, `${PREFIX}legacy home ${Date.now()}`, {
    node_type: 'project',
  });
  const taskId = '00000000-0000-4000-8000-000000000216';
  try {
    await page.route('**/api/data-health', (route) =>
      route.fulfill({
        json: {
          rules: [
            {
              rule: 'task_on_non_project',
              count: 1,
              items: [
                {
                  subject: { kind: 'task', id: taskId, label: 'legacy task', node_id: node.id },
                  related: [{ kind: 'node', id: node.id, label: 'legacy home' }],
                  more: 0,
                },
              ],
            },
          ],
        },
      }),
    );
    await page.goto('/data-health');
    const card = page.getByTestId('data-health-task_on_non_project');
    // Each card explains its rule and how to fix it.
    await expect(card).toContainText('Only Projects have tasks.');
    await expect(card).toContainText('To fix:');
    await expect(card.locator(`a[href="/nodes/${node.id}?task=${taskId}"]`)).toBeVisible();
    await expect(card.locator(`a[href="/nodes/${node.id}"]`)).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('"Move to a Project" fixes a task outside a Project in one step (#216)', async ({
  page,
  request,
}) => {
  await wipe(request);
  try {
    const stamp = Date.now();
    const project = await createProject(request, `${PREFIX}move target ${stamp}`);
    const article = await createNode(request, `${PREFIX}move article ${stamp}`);
    const home = await createProject(request, `${PREFIX}legacy home ${stamp}`);
    const task = await addTask(request, home.id, { title: `${PREFIX}to move ${stamp}` });

    // The API itself refuses the wrong destination.
    const refused = await request.patch(`/api/tasks/${task.id}`, {
      data: { node_id: article.id },
    });
    expect(refused.status()).toBe(422);

    await serveTasksOutsideAProject(page, request, home);
    await page.goto('/data-health');
    const card = page.getByTestId('data-health-task_on_non_project');
    const row = card.getByTestId('data-health-item').filter({ hasText: `${PREFIX}to move ${stamp}` });
    await row.getByRole('button', { name: /Move to a Project/ }).click();
    const picker = row.getByRole('combobox', { name: 'Move to a Project…' });
    await expect(picker).toBeFocused();
    await picker.fill(`${PREFIX}move`);
    // Projects only: the Article isn't offered.
    await expect(page.getByRole('option', { name: new RegExp(`move article ${stamp}`) })).toHaveCount(0);
    await expect(page.getByRole('option', { name: new RegExp(`move target ${stamp}`) })).toBeVisible();
    await picker.press('Enter');

    await expect(page.locator('[role="status"]').getByText(/Moved to/)).toBeVisible();
    await expect(page.getByText(`${PREFIX}to move ${stamp}`)).toHaveCount(0);
    await expect.poll(() => idsOn(request, project.id)).toEqual([task.id]);
    expect(await idsOn(request, home.id)).toEqual([]);
  } finally {
    await wipe(request);
  }
});

test('a task can be deleted from Data health, and Undo brings it back', async ({
  page,
  request,
}) => {
  await wipe(request);
  try {
    const stamp = Date.now();
    const title = `${PREFIX}delete one ${stamp}`;
    const home = await createProject(request, `${PREFIX}legacy home ${stamp}`);
    const task = await addTask(request, home.id, { title });
    await serveTasksOutsideAProject(page, request, home);
    await page.goto('/data-health');
    const card = page.getByTestId('data-health-task_on_non_project');
    const row = card.getByTestId('data-health-item').filter({ hasText: title });
    await row.getByRole('button', { name: 'Delete task' }).click();
    const confirm = row.getByRole('group', { name: 'Confirm delete' });
    await expect(confirm).toContainText(title);
    await confirm.getByRole('button', { name: 'Delete' }).click();
    await expect(page.getByText(title)).toHaveCount(0);
    expect(await idsOn(request, home.id)).not.toContain(task.id);

    // Undo restores it, and the report shows it again.
    await page.locator('[role="status"]').getByRole('button', { name: 'Undo' }).click();
    await expect(card.getByText(title)).toBeVisible();
    expect(await idsOn(request, home.id)).toContain(task.id);
  } finally {
    await wipe(request);
  }
});

test('"Delete finished" clears the done and cancelled tasks of a card, with Undo', async ({
  page,
  request,
}) => {
  await wipe(request);
  try {
    const stamp = Date.now();
    const home = await createProject(request, `${PREFIX}legacy home ${stamp}`);
    const make = (title: string, status?: string) =>
      addTask(request, home.id, status ? { title, status } : { title });
    const done = await make(`${PREFIX}done ${stamp}`, 'done');
    const cancelled = await make(`${PREFIX}cancelled ${stamp}`, 'cancelled');
    const open = await make(`${PREFIX}still open ${stamp}`);

    // The bulk button acts on what its card lists: this test's tasks only,
    // so it can never delete another spec's data in the shared DB.
    await serveTasksOutsideAProject(page, request, home);
    await page.goto('/data-health');
    const card = page.getByTestId('data-health-task_on_non_project');
    await card
      .getByTestId('delete-finished')
      .getByRole('button', { name: 'Delete the 2 finished tasks (done or cancelled)' })
      .click();
    const confirm = card.getByRole('group', { name: 'Confirm delete' });
    await expect(confirm).toContainText('Delete 2 finished tasks');
    await confirm.getByRole('button', { name: 'Delete' }).click();

    await expect(card.getByText(`${PREFIX}done ${stamp}`)).toHaveCount(0);
    await expect(card.getByText(`${PREFIX}cancelled ${stamp}`)).toHaveCount(0);
    await expect(card.getByText(`${PREFIX}still open ${stamp}`)).toBeVisible();
    let left = await idsOn(request, home.id);
    expect(left).not.toContain(done.id);
    expect(left).not.toContain(cancelled.id);
    expect(left).toContain(open.id);

    // One Undo brings both back.
    await page.locator('[role="status"]').getByRole('button', { name: 'Undo' }).click();
    await expect(card.getByText(`${PREFIX}done ${stamp}`)).toBeVisible();
    await expect(card.getByText(`${PREFIX}cancelled ${stamp}`)).toBeVisible();
    left = await idsOn(request, home.id);
    expect(left).toContain(done.id);
    expect(left).toContain(cancelled.id);
  } finally {
    await wipe(request);
  }
});

test('"Clear status" clears a status from a non-Project (#216)', async ({ page, request }) => {
  // A status on an Article can no longer be made; serve a report that has
  // one, pointing at a real Article, and check the fix's call goes through.
  const article = await createNode(request, `${PREFIX}status article ${Date.now()}`);
  try {
    await page.route('**/api/data-health', (route) =>
      route.fulfill({
        json: {
          rules: [
            {
              rule: 'status_on_non_project',
              count: 1,
              items: [{ subject: { kind: 'node', id: article.id, label: 'legacy status' }, related: [] }],
            },
          ],
        },
      }),
    );
    await page.goto('/data-health');
    const card = page.getByTestId('data-health-status_on_non_project');
    const cleared = page.waitForResponse(
      (r) => r.url().endsWith(`/api/nodes/${article.id}`) && r.request().method() === 'PUT',
    );
    await card.getByRole('button', { name: 'Clear status' }).click();
    const response = await cleared;
    expect(response.status()).toBe(200);
    // Exactly a clear: the status is sent as null, and every other field is
    // null too, which the API reads as "leave it unchanged".
    const sent = response.request().postDataJSON();
    expect(sent).toHaveProperty('project_status', null);
    for (const [field, value] of Object.entries(sent)) {
      if (field !== 'project_status') expect(value, field).toBeNull();
    }
    await expect(page.locator('[role="status"]').getByText('Status cleared')).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${article.id}`);
  }
});

test('the palette opens Data health', async ({ page }) => {
  await page.goto('/tasks/my-day');
  await expect(page.locator('main')).toBeVisible();
  await page.keyboard.press('ControlOrMeta+k');
  const dialog = page.getByRole('dialog');
  await dialog.getByPlaceholder(/Search/).fill('data health');
  await dialog.getByRole('button', { name: /Go to Data health/ }).click();
  await expect(page).toHaveURL(/\/data-health$/);
  await expect(page.getByRole('heading', { name: 'Data health' })).toBeVisible();
});

// ── Edge fixes (#228) ────────────────────────────────────────────────────────
// The API has refused contained References and loops since v3.1.0, so the
// spec can't create them: the report is stubbed with fake ids, and the fix
// calls are caught and answered, to check exactly what each button sends.

const node = (id: string, label: string) => ({ kind: 'node', id, label });
const link = (id: string, edge_type: string, source: string, target: string, source_owned: boolean) => ({
  id,
  edge_type,
  source,
  target,
  source_owned,
  target_owned: true,
});
const uuid = (n: number) => `00000000-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;

test('edge items convert, convert in bulk, and break a loop one link at a time', async ({ page }) => {
  const [area, refA, refB, p, q] = [1, 2, 3, 4, 5].map(uuid);
  const [linkA, linkB, hopPQ, hopQP] = [11, 12, 13, 14].map(uuid);
  await page.route('**/api/data-health', (route) =>
    route.fulfill({
      json: {
        rules: [
          {
            rule: 'contained_reference',
            count: 2,
            items: [
              {
                subject: node(refA, 'ref a'),
                related: [node(area, 'the area')],
                more: 0,
                links: [link(linkA, 'contains', area, refA, true)],
              },
              {
                subject: node(refB, 'ref b'),
                related: [node(area, 'the area')],
                more: 0,
                links: [link(linkB, 'contains', area, refB, true)],
              },
            ],
          },
          {
            rule: 'depends_on_cycle',
            count: 1,
            items: [
              {
                subject: node(p, 'proj p'),
                related: [node(q, 'proj q')],
                more: 0,
                // The second hop is someone else's: no button for it.
                links: [
                  link(hopPQ, 'depends_on', p, q, true),
                  link(hopQP, 'depends_on', q, p, false),
                ],
              },
            ],
          },
        ],
      },
    }),
  );
  const converts: { id: string; body: unknown }[] = [];
  await page.route('**/api/edges/*/convert', (route) => {
    converts.push({ id: route.request().url().split('/').at(-2)!, body: route.request().postDataJSON() });
    return route.fulfill({ status: 204 });
  });
  const deletes: string[] = [];
  await page.route('**/api/edges/*', (route) => {
    if (route.request().method() !== 'DELETE') return route.fallback();
    deletes.push(route.request().url().split('/').at(-1)!);
    return route.fulfill({ status: 204 });
  });

  await page.goto('/data-health');
  const refs = page.getByTestId('data-health-contained_reference');
  await expect(refs).toBeVisible();

  // One item.
  await refs
    .getByTestId('data-health-item')
    .first()
    .getByRole('button', { name: 'Make it a reference link' })
    .click();
  await expect.poll(() => converts.length).toBe(1);
  // A plain convert: the link keeps its direction (S7a added `reverse`).
  expect(converts[0]).toEqual({ id: linkA, body: { edge_type: 'references', reverse: false } });

  // All of them, after one confirmation.
  await refs.getByTestId('convert-all').getByRole('button', { name: 'Convert all 2' }).click();
  await refs.getByRole('group', { name: 'Confirm convert' }).getByRole('button', { name: 'Convert' }).click();
  await expect.poll(() => converts.length).toBe(3);
  expect(converts.slice(1).map((c) => c.id).sort()).toEqual([linkA, linkB].sort());

  // A loop: only the reader's own link can go.
  const loop = page.getByTestId('data-health-depends_on_cycle').getByTestId('loop-links');
  await expect(loop.getByRole('button', { name: /Remove the link/ })).toHaveCount(1);
  await loop.getByRole('button', { name: 'Remove the link proj p → proj q' }).click();
  await loop.getByRole('group', { name: 'Confirm remove' }).getByRole('button', { name: 'Remove' }).click();
  await expect.poll(() => deletes).toEqual([hopPQ]);
});
