import {
  test,
  expect,
  createProject,
  addTask,
  tasksOf,
  deleteNodesByPrefix,
  shareNode,
  localISO,
  E2E_SUBJECT,
} from './fixtures';
import type { APIRequestContext, Locator, Page } from '@playwright/test';

/**
 * Task dependencies (S5d, #338; ONTOLOGY §9.3, §6.4): a task can wait for
 * another task. A blocked task says so, and nothing else about it changes.
 */

// Every Project of this spec starts with this, so a retry can wipe what a
// failed attempt left (.claude/rules/e2e.md).
const PREFIX = 'e2e deps ';

test.beforeEach(async ({ request, otherRequest }) => {
  await deleteNodesByPrefix(request, PREFIX);
  await deleteNodesByPrefix(otherRequest, PREFIX);
});
test.afterEach(async ({ request, otherRequest }) => {
  await deleteNodesByPrefix(request, PREFIX);
  await deleteNodesByPrefix(otherRequest, PREFIX);
});

type Link = { id: string; title: string; status: string; node_title: string };
type Deps = { blocked_by: Link[]; blocks: Link[]; may_edit: boolean; not_shown: number };

const deps = (task: string) => `/api/tasks/${task}/dependencies`;

async function waitFor(request: APIRequestContext, task: string, other: string) {
  return request.post(deps(task), { data: { depends_on: other } });
}

async function blockedBy(request: APIRequestContext, node: string, task: string) {
  const rows = (await tasksOf(request, node)) as unknown as { id: string; blocked_by: number }[];
  return rows.find((t) => t.id === task)?.blocked_by;
}

/** Open a task's editor on its node's page and return its dependencies section. */
async function openDependencies(page: Page, taskId: string): Promise<Locator> {
  const row = page.locator(`[data-task-id="${taskId}"]`);
  await row.getByRole('button', { name: 'Edit task' }).click();
  const section = row.getByTestId('task-dependencies');
  await expect(section).toBeVisible();
  return section;
}

const picker = (section: Locator) => section.getByRole('combobox', { name: 'Wait for a task…' });

test('a task waits for another until that one is done', async ({ request }) => {
  const stamp = Date.now();
  const project = await createProject(request, `${PREFIX}api ${stamp}`);
  const paint = await addTask(request, project.id, { title: `paint ${stamp}` });
  const buy = await addTask(request, project.id, { title: `buy paint ${stamp}` });

  const added = await waitFor(request, paint.id, buy.id);
  expect(added.status()).toBe(201);
  const fresh: Deps = await added.json();
  expect(fresh.may_edit).toBe(true);
  expect(fresh.blocked_by.map((l) => l.id)).toEqual([buy.id]);
  expect(fresh.blocked_by[0].node_title).toBe(project.title);

  // The other side: what waits for it.
  const other: Deps = await (await request.get(deps(buy.id))).json();
  expect(other.blocks.map((l) => l.id)).toEqual([paint.id]);
  expect(other.blocked_by).toEqual([]);

  expect(await blockedBy(request, project.id, paint.id)).toBe(1);
  expect(await blockedBy(request, project.id, buy.id)).toBe(0);

  // Done lets it go; cancelled never does (§6.4).
  const state = (status: string) => request.patch(`/api/tasks/${buy.id}`, { data: { status } });
  await state('done');
  expect(await blockedBy(request, project.id, paint.id)).toBe(0);
  await state('cancelled');
  expect(await blockedBy(request, project.id, paint.id)).toBe(1);
  await state('open');

  // In the trash it neither shows nor counts; restored, it does again.
  await request.delete(`/api/tasks/${buy.id}`);
  expect(await blockedBy(request, project.id, paint.id)).toBe(0);
  expect(((await (await request.get(deps(paint.id))).json()) as Deps).blocked_by).toEqual([]);
  await request.post(`/api/tasks/${buy.id}/restore`);
  expect(await blockedBy(request, project.id, paint.id)).toBe(1);

  // A blocked task is completed like any other: nothing is enforced.
  const done = await request.patch(`/api/tasks/${paint.id}`, { data: { status: 'done' } });
  expect(done.ok()).toBeTruthy();
  await request.patch(`/api/tasks/${paint.id}`, { data: { status: 'open' } });

  const removed = await request.delete(`${deps(paint.id)}/${buy.id}`);
  expect(removed.status()).toBe(200);
  expect(((await removed.json()) as Deps).blocked_by).toEqual([]);
  expect((await request.delete(`${deps(paint.id)}/${buy.id}`)).status()).toBe(404);
  expect(await blockedBy(request, project.id, paint.id)).toBe(0);
});

test('a loop, a task with itself and a repeat are refused, and the picker offers none', async ({
  request,
}) => {
  const stamp = Date.now();
  const project = await createProject(request, `${PREFIX}loop ${stamp}`);
  const a = await addTask(request, project.id, { title: `a ${stamp}` });
  const b = await addTask(request, project.id, { title: `b ${stamp}` });
  const c = await addTask(request, project.id, { title: `c ${stamp}` });
  const finished = await addTask(request, project.id, { title: `finished ${stamp}` });
  await request.patch(`/api/tasks/${finished.id}`, { data: { status: 'done' } });

  expect((await waitFor(request, a.id, b.id)).status()).toBe(201);
  expect((await waitFor(request, b.id, c.id)).status()).toBe(201);

  const loop = await waitFor(request, c.id, a.id);
  expect(loop.status()).toBe(422);
  expect((await loop.json()).error).toContain('loop');
  const itself = await waitFor(request, a.id, a.id);
  expect(itself.status()).toBe(422);
  expect((await itself.json()).error).toContain('itself');
  expect((await waitFor(request, a.id, b.id)).status()).toBe(409);

  // The picker offers what can be added: not the task itself, not what it
  // already waits for, not what would make a loop, not a finished task.
  const offered = async (task: string) =>
    (
      (await (await request.get(`${deps(task)}/candidates?q=${stamp}`)).json()) as Link[]
    ).map((l) => l.id);
  expect(await offered(c.id)).toEqual([]);
  expect(await offered(a.id)).toEqual([c.id]);
  expect((await offered(b.id)).sort()).toEqual([]);
});

test('a task the caller cannot see reads like one that does not exist', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const mine = await createProject(request, `${PREFIX}mine ${stamp}`);
  const theirs = await createProject(otherRequest, `${PREFIX}theirs ${stamp}`);
  const task = await addTask(request, mine.id, { title: `mine ${stamp}` });
  const hidden = await addTask(otherRequest, theirs.id, { title: `hidden ${stamp}` });

  const body = async (r: Promise<import('@playwright/test').APIResponse>) => {
    const resp = await r;
    return { status: resp.status(), text: await resp.text() };
  };
  const nobody = '00000000-0000-4000-8000-00000000dead';
  const missing = await body(waitFor(request, task.id, nobody));
  expect(missing.status).toBe(404);
  expect(await body(waitFor(request, task.id, hidden.id))).toEqual(missing);
  // Reading and changing the hidden task itself: missing, too.
  expect((await request.get(deps(hidden.id))).status()).toBe(404);
  expect((await waitFor(request, hidden.id, task.id)).status()).toBe(404);
  expect((await request.get(`${deps(hidden.id)}/candidates`)).status()).toBe(404);
  // It is never offered.
  const offered: Link[] = await (
    await request.get(`${deps(task.id)}/candidates?q=${stamp}`)
  ).json();
  expect(offered).toEqual([]);

  // Shared for viewing: now it can be waited for, and read, not changed.
  await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'viewer');
  expect((await waitFor(request, task.id, hidden.id)).status()).toBe(201);
  const seen: Deps = await (await request.get(deps(hidden.id))).json();
  expect(seen.may_edit).toBe(false);
  expect(seen.blocks.map((l) => l.id)).toEqual([task.id]);
  expect((await waitFor(request, hidden.id, task.id)).status()).toBe(403);

  // Its owner can't see my Project, so my task is not in their list.
  const theirView: Deps = await (await otherRequest.get(deps(hidden.id))).json();
  expect(theirView.blocks).toEqual([]);
  expect((await otherRequest.get(deps(task.id))).status()).toBe(404);
});

test('a dependency is added and removed in the task editor, and the row says Blocked', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = await createProject(request, `${PREFIX}editor ${stamp}`);
  const paint = await addTask(request, project.id, { title: `paint ${stamp}` });
  const buy = await addTask(request, project.id, { title: `buy paint ${stamp}` });
  await addTask(request, project.id, { title: `clean brushes ${stamp}` });

  await page.goto(`/nodes/${project.id}`);
  const row = page.locator(`[data-task-id="${paint.id}"]`);
  const other = page.locator(`[data-task-id="${buy.id}"]`);
  await expect(row.getByTestId('task-blocked')).toHaveCount(0);

  // Which searches the picker sends.
  const asked: string[] = [];
  page.on('request', (r) => {
    if (r.url().includes('/dependencies/candidates')) asked.push(r.url());
  });

  let section = await openDependencies(page, paint.id);
  await expect(section).toContainText('Changes here apply at once.');
  // Nothing is asked before the field is focused; then the nearest tasks.
  // (Other specs' tasks may be offered too: count this test's own.)
  await expect(picker(section)).toBeVisible();
  expect(asked).toEqual([]);
  await picker(section).click();
  const mine = () => section.getByRole('option').filter({ hasText: String(stamp) });
  await expect(mine()).toHaveCount(2); // the other two tasks, not itself
  expect(asked.length, 'one search for one focus and click').toBe(1);

  // Escape closes the list, then clears the text, and only then is the
  // editor's.
  await picker(section).fill('buy');
  await expect(section.getByRole('option', { selected: true })).toContainText(
    `buy paint ${stamp}`,
  );
  await picker(section).press('Escape');
  await expect(section.getByRole('option')).toHaveCount(0);
  await expect(picker(section)).toHaveValue('buy');
  await picker(section).press('Escape');
  await expect(picker(section)).toHaveValue('');
  await expect(row.getByTestId('task-editor')).toBeVisible();

  await picker(section).fill('buy');
  await expect(section.getByRole('option', { selected: true })).toContainText(
    `buy paint ${stamp}`,
  );
  await picker(section).press('Enter');

  const list = section.getByTestId('blocked-by-list');
  await expect(list.getByTestId('dependency')).toHaveCount(1);
  await expect(list).toContainText(`buy paint ${stamp}`);
  // Same Project: its name is not repeated.
  await expect(list.getByTestId('dependency-project')).toHaveCount(0);
  // The picker stays, empty, and no longer offers it.
  await expect(picker(section)).toHaveValue('');
  await expect(picker(section)).toBeFocused();
  // It still has the focus, so only a click reopens its list.
  await picker(section).click();
  await expect(mine()).toHaveCount(1);
  await expect(mine()).toContainText(`clean brushes ${stamp}`);

  // Its title goes to the task: here, the row on this same page, with
  // no navigation (the page is already the right one).
  await list.getByRole('link', { name: `buy paint ${stamp}` }).click();
  await expect(other).toHaveClass(/focus-task-flash/);
  expect(new URL(page.url()).search).toBe('');
  await expect(row.getByTestId('task-editor')).toBeVisible();

  // Cancel leaves the dependency: it was applied at once. The list loads
  // again once the editor has gone, so a row rebuilt later (a reorder)
  // shows the new count.
  const reloaded = page.waitForResponse(
    (r) => r.url().endsWith(`/api/nodes/${project.id}/tasks`) && r.request().method() === 'GET',
  );
  await row.getByTestId('task-editor').getByRole('button', { name: 'Cancel' }).click();
  await reloaded;
  const chip = row.getByTestId('task-blocked');
  await expect(chip).toBeVisible();
  await expect(chip).toHaveAttribute('title', 'Blocked by 1 task');
  await expect(chip).toContainText('Blocked');
  expect(await blockedBy(request, project.id, paint.id)).toBe(1);

  // The other task says what waits for it, and offers no remove there.
  const blocks = (await openDependencies(page, buy.id)).getByTestId('blocks-list');
  await expect(blocks).toContainText(`paint ${stamp}`);
  await expect(blocks.getByRole('button')).toHaveCount(0);
  await other.getByTestId('task-editor').getByRole('button', { name: 'Cancel' }).click();

  // Done: the task is free again. Reopened: blocked again.
  await other.getByTitle('Toggle done').click();
  await expect(chip).toHaveCount(0);
  // Still listed in the editor, as done: linked, and no longer in the way.
  section = await openDependencies(page, paint.id);
  const doneItem = section.getByTestId('dependency');
  await expect(doneItem.getByTestId('task-state')).toHaveText('Done');
  await expect(doneItem.getByRole('link')).toHaveClass(/line-through/);
  await row.getByTestId('task-editor').getByRole('button', { name: 'Cancel' }).click();
  await page.getByRole('button', { name: /completed · show/ }).click();
  await other.getByTitle('Toggle done').click();
  await expect(chip).toBeVisible();

  // Remove it. The button goes with its row; the keyboard stays with the
  // editor, so Escape closes the editor and does not leave the page.
  section = await openDependencies(page, paint.id);
  const remove = section.getByRole('button', { name: `Stop waiting for buy paint ${stamp}` });
  await remove.click();
  await expect(section.getByTestId('blocked-by-list')).toHaveCount(0);
  await expect(section).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(row.getByTestId('task-editor')).toHaveCount(0);
  expect(new URL(page.url()).pathname).toBe(`/nodes/${project.id}`);
  await expect(chip).toHaveCount(0);
  expect(await blockedBy(request, project.id, paint.id)).toBe(0);
});

test('a blocked task keeps its place in My Day and says so', async ({ page, request }) => {
  const stamp = Date.now();
  const project = await createProject(request, `${PREFIX}day ${stamp}`);
  const elsewhere = await createProject(request, `${PREFIX}day other ${stamp}`);
  const title = `send the invoice ${stamp}`;
  const task = await addTask(request, project.id, { title, focus_date: localISO(0) });
  const first = await addTask(request, elsewhere.id, { title: `agree the price ${stamp}` });
  await addTask(request, elsewhere.id, { title: `get the order number ${stamp}` });
  expect((await waitFor(request, task.id, first.id)).status()).toBe(201);

  await page.goto('/tasks/my-day');
  // In Today, where an unblocked task with today's focus date is.
  const row = page.getByTestId('zone-today').locator(`[data-task-id="${task.id}"]`);
  await expect(row).toBeVisible();
  await expect(row.getByTestId('task-blocked')).toHaveAttribute('title', 'Blocked by 1 task');

  // Its editor names the other Project, since it is not this task's own.
  await row.locator('button[title="Edit task"]').click();
  const item = row.getByTestId('blocked-by-list').getByTestId('dependency');
  await expect(item).toContainText(`agree the price ${stamp}`);
  await expect(item.getByTestId('dependency-project')).toHaveText(elsewhere.title);

  // A second one, picked with the mouse, from My Day's editor.
  const section = row.getByTestId('task-dependencies');
  await picker(section).fill('order number');
  const option = section.getByRole('option').filter({ hasText: `get the order number ${stamp}` });
  await expect(option).toBeVisible();
  await option.click();
  await expect(row.getByTestId('blocked-by-list').getByTestId('dependency')).toHaveCount(2);

  await row.getByTestId('task-editor').getByRole('button', { name: 'Cancel' }).click();
  await expect(row.getByTestId('task-blocked')).toHaveAttribute('title', 'Blocked by 2 tasks');
  await expect(row).toBeVisible(); // still in Today

  // Ticked off like any other task.
  await row.getByTitle('Toggle done').click();
  await expect
    .poll(async () => (await tasksOf(request, project.id)).find((t) => t.id === task.id)?.status)
    .toBe('done');
});

test('someone who may only view a task sees what blocks it and no controls', async ({
  page,
  otherRequest,
}) => {
  const stamp = Date.now();
  const theirs = await createProject(otherRequest, `${PREFIX}viewed ${stamp}`);
  const hidden = await createProject(otherRequest, `${PREFIX}unseen ${stamp}`);
  const task = await addTask(otherRequest, theirs.id, { title: `ship it ${stamp}` });
  const seen = await addTask(otherRequest, theirs.id, { title: `test it ${stamp}` });
  const unseen = await addTask(otherRequest, hidden.id, { title: `secret step ${stamp}` });
  expect((await waitFor(otherRequest, task.id, seen.id)).status()).toBe(201);
  expect((await waitFor(otherRequest, task.id, unseen.id)).status()).toBe(201);
  await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'viewer');

  await page.goto(`/nodes/${theirs.id}`);
  const row = page.locator(`[data-task-id="${task.id}"]`);
  // One of its two prerequisites is on a Project this reader can't see.
  await expect(row.getByTestId('task-blocked')).toHaveAttribute('title', 'Blocked by 1 task');

  const section = await openDependencies(page, task.id);
  await expect(section.getByTestId('dependency')).toHaveCount(1);
  await expect(section).toContainText(`test it ${stamp}`);
  await expect(section).not.toContainText('secret step');
  await expect(section.getByRole('combobox')).toHaveCount(0);
  await expect(section.getByRole('button')).toHaveCount(0);
  await expect(section).not.toContainText('apply at once');
  // A viewer is told nothing of what they aren't shown: not even a count.
  await expect(section.getByTestId('dependencies-not-shown')).toHaveCount(0);
  const read = await page.request.get(deps(task.id));
  expect(((await read.json()) as Deps).not_shown).toBe(0);
});

test('an editor is told a task waits for something they are not shown, and removes it', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const theirs = await createProject(otherRequest, `${PREFIX}edited ${stamp}`);
  const hidden = await createProject(otherRequest, `${PREFIX}private ${stamp}`);
  const task = await addTask(otherRequest, theirs.id, { title: `publish ${stamp}` });
  const unseen = await addTask(otherRequest, hidden.id, { title: `secret review ${stamp}` });
  expect((await waitFor(otherRequest, task.id, unseen.id)).status()).toBe(201);
  await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'editor');

  // Told how many, never which; and it is not counted as blocking.
  const before: Deps = await (await request.get(deps(task.id))).json();
  expect(before.blocked_by).toEqual([]);
  expect(before.not_shown).toBe(1);
  expect(await blockedBy(request, theirs.id, task.id)).toBe(0);
  // It can't be removed by guessing: its id answers like no link at all.
  expect((await request.delete(`${deps(task.id)}/${unseen.id}`)).status()).toBe(404);

  await page.goto(`/nodes/${theirs.id}`);
  const row = page.locator(`[data-task-id="${task.id}"]`);
  await expect(row.getByTestId('task-blocked')).toHaveCount(0);
  const section = await openDependencies(page, task.id);
  const line = section.getByTestId('dependencies-not-shown');
  await expect(line).toContainText('Also waits for 1 more task not shown here');
  await expect(section).not.toContainText('secret review');
  await line.getByRole('button', { name: 'Stop waiting for the task not shown' }).click();
  await expect(line).toHaveCount(0);

  const after: Deps = await (await request.get(deps(task.id))).json();
  expect(after.not_shown).toBe(0);
  // Its owner sees the link is gone.
  const theirView: Deps = await (await otherRequest.get(deps(task.id))).json();
  expect(theirView.blocked_by).toEqual([]);
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the dependencies fit the task editor', async ({ page, request }) => {
    const stamp = Date.now();
    const project = await createProject(request, `${PREFIX}phone ${stamp}`);
    const task = await addTask(request, project.id, { title: `narrow ${stamp}` });
    const long = await addTask(request, project.id, {
      title: `a prerequisite with a long title that has to wrap on a phone ${stamp}`,
    });
    expect((await waitFor(request, task.id, long.id)).status()).toBe(201);

    await page.goto(`/nodes/${project.id}`);
    const section = await openDependencies(page, task.id);
    const remove = section.getByRole('button', { name: /^Stop waiting for/ });
    await expect(remove).toBeVisible();
    // The remove button is on screen, not pushed out by the title.
    const box = await remove.boundingBox();
    expect(box && box.x + box.width).toBeLessThanOrEqual(375);
    await picker(section).click();
    await expect(picker(section)).toBeFocused();
    const over = await page.locator('main').evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(over).toBeLessThanOrEqual(1);
  });
});
