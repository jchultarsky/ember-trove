import { test, expect, shareNode, E2E_SUBJECT } from './fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

/**
 * The cascade in the browser (S6b and S6c, #326): the confirmation before
 * closing a project closes what is under it, the notice on a closed
 * project that still has something open, and Data health's fixes. The
 * rules themselves are held through the API in cascade.spec.ts.
 */

const OTHER_SUBJECT = 'e2e-other-user';
const PREFIX = 'e2e cascui ';

async function project(request: APIRequestContext, title: string, label?: string) {
  const resp = await request.post('/api/nodes', {
    data: {
      title,
      node_type: 'project',
      body: '',
      metadata: {},
      status: 'draft',
      ...(label ? { project_status: label } : {}),
    },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

async function addTask(request: APIRequestContext, nodeId: string, title: string) {
  const resp = await request.post(`/api/nodes/${nodeId}/tasks`, { data: { title } });
  expect(resp.ok(), `task ${title}`).toBeTruthy();
  return resp.json();
}

async function under(request: APIRequestContext, home: string, child: string) {
  const resp = await request.post('/api/edges', {
    data: { source_id: home, target_id: child, edge_type: 'contains' },
  });
  expect(resp.ok(), 'contains edge').toBeTruthy();
}

const stateOf = async (request: APIRequestContext, id: string): Promise<string> =>
  (await (await request.get(`/api/nodes/${id}`)).json()).project_state;

const statusOf = async (request: APIRequestContext, node: string, task: string) =>
  ((await (await request.get(`/api/nodes/${node}/tasks`)).json()) as { id: string; status: string }[]).find(
    (t) => t.id === task,
  )?.status;

/** Close `id` through the API, confirming whatever its plan is. */
async function closeByApi(request: APIRequestContext, id: string, state: string) {
  const plan = await (await request.get(`/api/nodes/${id}/project-state/cascade?state=${state}`)).json();
  const resp = await request.put(`/api/nodes/${id}/project-state`, {
    data: { state, confirm: plan.token },
  });
  expect(resp.ok(), `close ${state}`).toBeTruthy();
}

async function wipe(request: APIRequestContext) {
  for (let round = 0; round < 5; round++) {
    const hits: { id: string; title: string }[] = await (
      await request.get(`/api/nodes/lookup?q=${encodeURIComponent(PREFIX.trim())}&limit=20`)
    ).json();
    const mine = hits.filter((n) => n.title.startsWith(PREFIX));
    if (mine.length === 0) break;
    for (const n of mine) await request.delete(`/api/nodes/${n.id}`);
  }
}

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(otherRequest);
  await wipe(request);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(otherRequest);
  await wipe(request);
});

const dialog = (page: Page) => page.getByRole('alertdialog', { name: 'Confirm closing a project' });

/** Open the state chip and choose `state`. */
async function choose(page: Page, state: string) {
  const chip = page.getByTestId('project-state-chip');
  await expect(chip).toHaveAttribute('aria-disabled', 'false');
  await chip.click();
  await page.getByTestId(`project-state-option-${state}`).click();
}

test('closing a project shows what closes with it, and going back changes nothing', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const master = await project(request, `${PREFIX}kitchen ${stamp}`);
  const child = await project(request, `${PREFIX}cabinets ${stamp}`);
  await under(request, master.id, child.id);
  const mine = await addTask(request, master.id, `${PREFIX}order the tiles ${stamp}`);
  const onChild = await addTask(request, child.id, `${PREFIX}measure twice ${stamp}`);
  // Someone else's task on the master, and their project under it that
  // this reader may only view.
  await shareNode(request, master.id, OTHER_SUBJECT, 'editor');
  const theirs = await addTask(otherRequest, master.id, `${PREFIX}their quote ${stamp}`);
  const viewed = await project(otherRequest, `${PREFIX}electrics ${stamp}`);
  await under(otherRequest, master.id, viewed.id);
  await shareNode(otherRequest, viewed.id, E2E_SUBJECT, 'viewer');

  await page.goto(`/nodes/${master.id}`);
  const chip = page.getByTestId('project-state-chip');
  await expect(chip).toHaveAttribute('data-state', 'active');
  await choose(page, 'complete');

  // The plan, before anything is written.
  const box = dialog(page);
  await expect(box).toBeVisible();
  await expect(box.getByTestId('cascade-heading')).toHaveText(`Complete “${master.title}”?`);
  await expect(box.getByTestId('cascade-projects')).toContainText('Sub-projects that become complete (1)');
  await expect(box.getByTestId('cascade-projects')).toContainText(child.title);
  const tasks = box.getByTestId('cascade-tasks');
  await expect(tasks).toContainText('Tasks that are cancelled (3, 1 of them other people’s)');
  for (const t of [mine, onChild, theirs]) await expect(tasks).toContainText(t.title);
  await expect(tasks).toContainText(`On ${child.title}`);
  await expect(box.getByTestId('cascade-skipped')).toContainText(viewed.title);
  // The safe button has the keyboard, and the chip has not moved.
  await expect(box.getByTestId('cascade-back')).toBeFocused();
  await expect(chip).toHaveAttribute('data-state', 'active');
  expect(await stateOf(request, master.id)).toBe('active');

  // Going back, by the button and by Escape, writes nothing and stays on
  // the page.
  await box.getByTestId('cascade-back').click();
  await expect(box).toHaveCount(0);
  await expect(chip).toHaveAttribute('data-state', 'active');
  await choose(page, 'complete');
  await expect(box).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(box).toHaveCount(0);
  expect(new URL(page.url()).pathname).toBe(`/nodes/${master.id}`);
  // Escape is the dialog's wherever the focus went: after a click on its
  // text it still goes back, and never reaches the page behind (whose
  // Escape leaves for the node list, and whose `d` duplicates).
  await choose(page, 'complete');
  await box.getByTestId('cascade-heading').click();
  await page.keyboard.press('d');
  await page.keyboard.press('Escape');
  await expect(box).toHaveCount(0);
  expect(new URL(page.url()).pathname).toBe(`/nodes/${master.id}`);
  expect(await stateOf(request, master.id)).toBe('active');
  expect(await statusOf(request, master.id, mine.id)).toBe('open');

  // Confirmed: the project and what was listed close; the rest stays.
  await choose(page, 'complete');
  await expect(box.getByTestId('cascade-confirm')).toHaveText(/Complete and close these/);
  await box.getByTestId('cascade-confirm').click();
  await expect(box).toHaveCount(0);
  await expect(chip).toContainText(/Complete since/);
  await expect.poll(() => stateOf(request, child.id)).toBe('complete');
  expect(await stateOf(otherRequest, viewed.id)).toBe('active');
  for (const [node, t] of [
    [master.id, mine.id],
    [master.id, theirs.id],
    [child.id, onChild.id],
  ]) {
    expect(await statusOf(request, node, t), t).toBe('cancelled');
  }
  // Nothing is left to say on the page (the sub-project that stays open
  // is not this reader's to close, so it is not counted), and History has
  // the record. The notice asks the server first: wait for its answer.
  await page.waitForLoadState('networkidle');
  await expect(page.getByTestId('still-open-notice')).toHaveCount(0);
  const section = page.getByTestId('node-activity');
  const toggle = section.locator('button[aria-expanded]').first();
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
  await expect(section.getByText('closed what was under it')).toHaveCount(1);
  await expect(section.getByText('1 sub-project, 2 tasks')).toHaveCount(1);
  // On the sub-project's page: where its closing came from.
  await page.goto(`/nodes/${child.id}`);
  await expect(page.getByTestId('project-state-chip')).toContainText(/Complete since/);
  const childHistory = page.getByTestId('node-activity');
  const childToggle = childHistory.locator('button[aria-expanded]').first();
  if ((await childToggle.getAttribute('aria-expanded')) !== 'true') await childToggle.click();
  await expect(childHistory.getByText(`with “${master.title}” · 1 task`)).toHaveCount(1);
});

test('a project with nothing under it closes without a question', async ({ page, request }) => {
  const p = await project(request, `${PREFIX}empty ${Date.now()}`);
  await page.goto(`/nodes/${p.id}`);
  await choose(page, 'cancelled');
  await expect(page.getByTestId('project-state-chip')).toContainText(/Cancelled since/);
  await expect(dialog(page)).toHaveCount(0);
  expect(await stateOf(request, p.id)).toBe('cancelled');
});

test('leaving the page withdraws the question', async ({ page, request }) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}left ${stamp}`);
  const task = await addTask(request, p.id, `${PREFIX}still mine ${stamp}`);
  // In-app navigation, so Back and Forward move inside the running app
  // (a `goto` would reload it, and a reload has no dialog to withdraw).
  await page.goto(`/nodes/${p.id}`);
  await expect(page.getByTestId('project-state-chip')).toBeVisible();
  await page.getByRole('navigation').getByRole('button', { name: /Dashboard/ }).click();
  await expect(page).toHaveURL(/\/dashboard$/);
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/nodes/${p.id}$`));
  await choose(page, 'complete');
  const box = dialog(page);
  await expect(box).toBeVisible();
  // Forward to the dashboard with the dialog open: it does not stay up
  // over another page, and nothing was closed.
  await page.goForward();
  await expect(page).toHaveURL(/\/dashboard$/);
  await expect(box).toHaveCount(0);
  expect(await stateOf(request, p.id)).toBe('active');
  expect(await statusOf(request, p.id, task.id)).toBe('open');
});

test('the confirmation shows the plan again when it changed underneath', async ({ page, request }) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}moving ${stamp}`);
  const first = await addTask(request, p.id, `${PREFIX}first ${stamp}`);
  await page.goto(`/nodes/${p.id}`);
  await choose(page, 'cancelled');
  const box = dialog(page);
  await expect(box.getByTestId('cascade-tasks')).toContainText('Tasks that are cancelled (1)');

  // A task is added while the dialog is open (another tab, another person).
  const late = await addTask(request, p.id, `${PREFIX}added meanwhile ${stamp}`);
  await box.getByTestId('cascade-confirm').click();
  // Nothing was closed. The dialog says so and shows what is there now.
  await expect(box.getByTestId('cascade-changed')).toBeVisible();
  await expect(box.getByTestId('cascade-tasks')).toContainText('Tasks that are cancelled (2)');
  await expect(box.getByTestId('cascade-tasks')).toContainText(late.title);
  expect(await stateOf(request, p.id)).toBe('active');
  expect(await statusOf(request, p.id, first.id)).toBe('open');

  await box.getByTestId('cascade-confirm').click();
  await expect(box).toHaveCount(0);
  await expect(page.getByTestId('project-state-chip')).toContainText(/Cancelled since/);
  expect(await statusOf(request, p.id, late.id)).toBe('cancelled');
});

test('a closed project says what is still open under it and offers to close it', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}left open ${stamp}`);
  await closeByApi(request, p.id, 'complete');
  await page.goto(`/nodes/${p.id}`);
  await expect(page.getByTestId('project-state-chip')).toContainText(/Complete since/);
  // Nothing open: nothing said.
  await expect(page.getByTestId('still-open-notice')).toHaveCount(0);

  // A task arrives on the closed project. Nothing closes it by itself.
  const task = await addTask(request, p.id, `${PREFIX}arrived later ${stamp}`);
  await page.reload();
  const notice = page.getByTestId('still-open-notice');
  await expect(notice).toContainText('This project is complete, and 1 task under it is still open.');
  expect(await statusOf(request, p.id, task.id)).toBe('open');

  // "Close them" asks with the same confirmation, worded for a project
  // that is already closed. Going back leaves the notice and the task.
  await notice.getByTestId('still-open-close').click();
  const box = dialog(page);
  await expect(box.getByTestId('cascade-heading')).toHaveText(
    `Close what is still open under “${p.title}”?`,
  );
  await expect(box.getByTestId('cascade-tasks')).toContainText(task.title);
  await expect(box.getByTestId('cascade-confirm')).toHaveText(/Close these/);
  await box.getByTestId('cascade-back').click();
  await expect(box).toHaveCount(0);
  await expect(notice).toBeVisible();
  // The keyboard is back on the button that asked.
  await expect(notice.getByTestId('still-open-close')).toBeFocused();
  expect(await statusOf(request, p.id, task.id)).toBe('open');

  await notice.getByTestId('still-open-close').click();
  await box.getByTestId('cascade-confirm').click();
  await expect(box).toHaveCount(0);
  await expect(notice).toHaveCount(0);
  await expect.poll(() => statusOf(request, p.id, task.id)).toBe('cancelled');
  await expect(page.getByTestId('project-state-chip')).toContainText(/Complete since/);
});

test('a save that fails says so in the dialog, which keeps the keyboard', async ({ page, request }) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}failing ${stamp}`);
  const task = await addTask(request, p.id, `${PREFIX}kept ${stamp}`);
  await page.goto(`/nodes/${p.id}`);
  await choose(page, 'complete');
  const box = dialog(page);
  await expect(box).toBeVisible();
  // The confirmed save fails (not a 412: nothing to show again).
  await page.route('**/api/nodes/*/project-state', (route) =>
    route.request().method() === 'PUT'
      ? route.fulfill({ status: 500, json: { error: 'no' } })
      : route.fallback(),
  );
  const confirm = box.getByTestId('cascade-confirm');
  await confirm.focus();
  await page.keyboard.press('Enter');
  await expect(box.getByTestId('cascade-error')).toContainText('Couldn’t save');
  // Still open, still the plan, and the focus has not fallen to the page.
  await expect(box.getByTestId('cascade-tasks')).toContainText(task.title);
  await expect(confirm).toBeFocused();
  await expect(confirm).toHaveAttribute('aria-disabled', 'false');
  await page.unroute('**/api/nodes/*/project-state');
  await page.keyboard.press('Escape');
  await expect(box).toHaveCount(0);
  await expect(page.getByTestId('project-state-chip')).toHaveAttribute('data-state', 'active');
  expect(await stateOf(request, p.id)).toBe('active');
  expect(await statusOf(request, p.id, task.id)).toBe('open');
});

test('an offer that is out of date closes nothing and reads again', async ({ page, request }) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}stale ${stamp}`);
  await closeByApi(request, p.id, 'complete');
  const task = await addTask(request, p.id, `${PREFIX}stale task ${stamp}`);
  await page.goto(`/nodes/${p.id}`);
  const notice = page.getByTestId('still-open-notice');
  await expect(notice).toBeVisible();
  const toast = (text: RegExp) =>
    page.locator('[role="alert"], [role="status"]').filter({ hasText: text });

  // Closed elsewhere meanwhile (another tab): nothing to ask about. The
  // page says so and the notice goes.
  await closeByApi(request, p.id, 'complete');
  await notice.getByTestId('still-open-close').click();
  await expect(toast(/Nothing is left to close under it/)).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);
  await expect(notice).toHaveCount(0);

  // Re-opened elsewhere, with a new task, while this page still shows it
  // complete: "Close them" must not close the project again.
  const q = await project(request, `${PREFIX}reopened ${stamp}`);
  await closeByApi(request, q.id, 'complete');
  await addTask(request, q.id, `${PREFIX}reopened task ${stamp}`);
  await page.goto(`/nodes/${q.id}`);
  await expect(notice).toBeVisible();
  expect(
    (await request.put(`/api/nodes/${q.id}/project-state`, { data: { state: 'active' } })).ok(),
  ).toBeTruthy();
  await notice.getByTestId('still-open-close').click();
  await expect(toast(/This project is not complete any more/)).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);
  expect(await stateOf(request, q.id)).toBe('active');
  expect(await statusOf(request, p.id, task.id)).toBe('cancelled');
});

test('a sub-project is not re-opened under a closed master, and the chip says why', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const master = await project(request, `${PREFIX}closed master ${stamp}`);
  const child = await project(request, `${PREFIX}closed child ${stamp}`);
  await under(request, master.id, child.id);
  await closeByApi(request, master.id, 'complete');

  await page.goto(`/nodes/${child.id}`);
  const chip = page.getByTestId('project-state-chip');
  await expect(chip).toContainText(/Complete since/);
  await choose(page, 'active');
  await expect(
    page.locator('[role="alert"], [role="status"]').filter({ hasText: /its master project is closed/ }),
  ).toBeVisible();
  await expect(chip).toHaveAttribute('data-state', 'complete');
  expect(await stateOf(request, child.id)).toBe('complete');
});

test('Data health closes what is still open, and its label fix asks before it closes', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  // Closed, with a task that arrived afterwards.
  const closed = await project(request, `${PREFIX}dh closed ${stamp}`);
  await closeByApi(request, closed.id, 'cancelled');
  const left = await addTask(request, closed.id, `${PREFIX}dh left ${stamp}`);
  // Labelled "Done" but active, with an open task: "Mark complete" would
  // cancel that task, so it asks.
  const labelled = await project(request, `${PREFIX}dh labelled ${stamp}`, 'Done');
  const open = await addTask(request, labelled.id, `${PREFIX}dh open ${stamp}`);

  await page.goto('/data-health');
  const item = (title: string) => page.getByTestId('data-health-item').filter({ hasText: title });
  await expect(item(closed.title).getByTestId('still-open')).toHaveText(
    'cancelled, with 1 task still open',
  );
  await item(closed.title).getByTestId('fix-close-still-open').click();
  const box = dialog(page);
  await expect(box.getByTestId('cascade-heading')).toHaveText(
    `Close what is still open under “${closed.title}”?`,
  );
  await box.getByTestId('cascade-confirm').click();
  await expect(box).toHaveCount(0);
  await expect(item(closed.title)).toHaveCount(0);
  expect(await statusOf(request, closed.id, left.id)).toBe('cancelled');

  // The label fix: going back leaves the item and the project as they were.
  await item(labelled.title).getByTestId('fix-state-complete').click();
  await expect(box.getByTestId('cascade-heading')).toHaveText(`Complete “${labelled.title}”?`);
  await expect(box.getByTestId('cascade-tasks')).toContainText(open.title);
  await box.getByTestId('cascade-back').click();
  await expect(box).toHaveCount(0);
  await expect(item(labelled.title).getByTestId('fix-state-complete')).toBeEnabled();
  expect(await stateOf(request, labelled.id)).toBe('active');
  // The keyboard is back on the button that asked.
  await expect(item(labelled.title).getByTestId('fix-state-complete')).toBeFocused();
  // "Keep active" declares the state it has: no question, the item goes.
  await item(labelled.title).getByTestId('fix-state-active').click();
  await expect(item(labelled.title)).toHaveCount(0);
  await expect(box).toHaveCount(0);
  expect(await statusOf(request, labelled.id, open.id)).toBe('open');

  // The other fix for something still open under a closed project:
  // make the project active again. Nothing under it is touched.
  const reopen = await project(request, `${PREFIX}dh reopen ${stamp}`);
  await closeByApi(request, reopen.id, 'complete');
  const stays = await addTask(request, reopen.id, `${PREFIX}dh stays ${stamp}`);
  await page.reload();
  await item(reopen.title).getByTestId('fix-state-active').click();
  await expect(item(reopen.title)).toHaveCount(0);
  expect(await stateOf(request, reopen.id)).toBe('active');
  expect(await statusOf(request, reopen.id, stays.id)).toBe('open');
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the confirmation fits the screen and its buttons stay in reach', async ({ page, request }) => {
    const stamp = Date.now();
    // With a stretch that has no space to wrap at, as a pasted URL has.
    const long =
      'cabinets, counters and https://example.com/the/long-delayed/lighting-plan/for-the-back-room';
    const master = await project(request, `${PREFIX}${long} ${stamp}`);
    for (let n = 0; n < 3; n++) {
      const child = await project(request, `${PREFIX}sub ${n} ${long} ${stamp}`);
      await under(request, master.id, child.id);
      for (let t = 0; t < 7; t++) {
        await addTask(request, child.id, `${PREFIX}task ${n}.${t} ${long} ${stamp}`);
      }
    }
    await page.goto(`/nodes/${master.id}`);
    await choose(page, 'complete');
    const box = dialog(page);
    await expect(box).toBeVisible();
    await expect(box.getByTestId('cascade-tasks')).toContainText('+2 more');
    // Inside the screen, with the list scrolling and not the page.
    const edges = await box.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, w: innerWidth, h: innerHeight };
    });
    expect(edges.left).toBeGreaterThanOrEqual(0);
    expect(edges.right).toBeLessThanOrEqual(edges.w);
    expect(edges.top).toBeGreaterThanOrEqual(0);
    expect(edges.bottom).toBeLessThanOrEqual(edges.h);
    const over = await box.evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(over, 'the dialog scrolls sideways').toBeLessThanOrEqual(1);
    const listOver = await box
      .getByTestId('cascade-plan')
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(listOver, 'the list scrolls sideways').toBeLessThanOrEqual(1);
    await expect(box.getByTestId('cascade-confirm')).toBeInViewport({ ratio: 1 });
    await expect(box.getByTestId('cascade-back')).toBeInViewport({ ratio: 1 });
    await box.getByTestId('cascade-back').click();

    // The notice on a closed project wraps instead of pushing the page wide.
    const closed = await project(request, `${PREFIX}closed ${long} ${stamp}`);
    await closeByApi(request, closed.id, 'complete');
    await addTask(request, closed.id, `${PREFIX}late ${stamp}`);
    await page.goto(`/nodes/${closed.id}`);
    await expect(page.getByTestId('still-open-notice')).toBeVisible();
    const main = page.locator('main');
    const wide = await main.evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(wide, 'the page scrolls sideways').toBeLessThanOrEqual(1);
    await expect(page.getByTestId('still-open-close')).toBeInViewport({ ratio: 1 });
  });
});
