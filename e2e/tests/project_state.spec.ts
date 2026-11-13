import { test, expect, createNode, shareNode } from './fixtures';
import type { APIRequestContext } from '@playwright/test';

/**
 * Project states (S6a, #320; ONTOLOGY §4, §4.1): a Project is active,
 * parked, complete or cancelled. The state is declared, never read from the
 * status label, and only an active project asks for attention.
 */

const OTHER_SUBJECT = 'e2e-other-user';
const PREFIX = 'e2e pstate ';

async function declare(request: APIRequestContext, nodeId: string, state: string) {
  const resp = await request.put(`/api/nodes/${nodeId}/project-state`, { data: { state } });
  expect(resp.ok(), `declare ${state}`).toBeTruthy();
  return resp.json();
}

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

/** Empty the caller's Focus: it belongs to the identity, not to a node. */
async function wipeFocus(request: APIRequestContext) {
  const resp = await request.get('/api/focus');
  expect(resp.ok()).toBeTruthy();
  for (const entry of await resp.json()) {
    await request.delete(`/api/focus/${entry.focus.node_id}`);
  }
}

/** Retry-safe (`.claude/rules/e2e.md`): remove what a crashed earlier
 * attempt left behind before starting, and everything after. */
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
  await wipe(request);
  await wipeFocus(request);
  await wipeFocus(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipeFocus(request);
  await wipeFocus(otherRequest);
});

const focusTitles = async (request: APIRequestContext): Promise<string[]> =>
  (await (await request.get('/api/focus')).json()).map(
    (e: { project: { title: string } }) => e.project.title,
  );

test('a state is declared by an editor, dated, and written to History', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}declared ${stamp}`, 'Active');
  // A new project is active, and nobody has declared that.
  expect(p.project_state).toBe('active');
  expect(p.project_state_at).toBeNull();
  // Nothing but a Project has a state.
  const article = await createNode(request, `${PREFIX}article ${stamp}`);
  expect(article.project_state).toBeNull();

  const parked = await declare(request, p.id, 'parked');
  expect(parked.project_state).toBe('parked');
  expect(parked.project_state_at).toBeTruthy();
  expect(parked.project_status, 'the label is left alone').toBe('Active');
  // Sent again (a stale tab): the same date, the same version.
  const again = await declare(request, p.id, 'parked');
  expect(again.project_state_at).toBe(parked.project_state_at);
  expect(again.updated_at).toBe(parked.updated_at);

  // A label never sets the state again: "Done" does not complete it.
  const relabelled = await request.put(`/api/nodes/${p.id}`, { data: { project_status: 'Done' } });
  expect((await relabelled.json()).project_state).toBe('parked');

  await declare(request, p.id, 'complete');
  // A copy is a new, active project: it doesn't start out labelled "Done".
  const copy = await (await request.post(`/api/nodes/${p.id}/duplicate`)).json();
  expect(copy.project_state).toBe('active');
  expect(copy.project_state_at).toBeNull();
  expect(copy.project_status).toBe('Active');
  const history: { action: string; metadata: { from?: string; to?: string } }[] = await (
    await request.get(`/api/nodes/${p.id}/activity`)
  ).json();
  const changes = history
    .filter((e) => e.action === 'project_state_changed')
    .map((e) => `${e.metadata.from} > ${e.metadata.to}`)
    .sort();
  expect(changes).toEqual(['active > parked', 'parked > complete']);

  // Only a Project, and only one of the four states.
  const onArticle = await request.put(`/api/nodes/${article.id}/project-state`, {
    data: { state: 'parked' },
  });
  expect(onArticle.status()).toBe(422);
  const noSuchState = await request.put(`/api/nodes/${p.id}/project-state`, {
    data: { state: 'on_hold' },
  });
  expect(noSuchState.status()).toBe(422);

  // A viewer may read the state and not declare one; a stranger learns
  // nothing, not even that the other node is an Article.
  await shareNode(request, p.id, OTHER_SUBJECT, 'viewer');
  expect((await (await otherRequest.get(`/api/nodes/${p.id}`)).json()).project_state).toBe(
    'complete',
  );
  for (const id of [p.id, article.id]) {
    const refused = await otherRequest.put(`/api/nodes/${id}/project-state`, {
      data: { state: 'active' },
    });
    expect(refused.status(), id).toBe(403);
  }
  expect((await (await request.get(`/api/nodes/${p.id}`)).json()).project_state).toBe('complete');
});

test('Focus shows active projects: parking hides one, closing removes it', async ({ request }) => {
  const stamp = Date.now();
  const kept = await project(request, `${PREFIX}kept ${stamp}`);
  const moving = await project(request, `${PREFIX}moving ${stamp}`);
  for (const [p, why] of [
    [kept, 'the steady one'],
    [moving, 'needs me this week'],
  ] as const) {
    const put = await request.put(`/api/focus/${p.id}`, { data: { why_now: why } });
    expect(put.ok()).toBeTruthy();
  }
  expect(await focusTitles(request)).toEqual([kept.title, moving.title]);

  // Parked: out of Focus, and it can't be put there.
  await declare(request, moving.id, 'parked');
  expect(await focusTitles(request)).toEqual([kept.title]);
  const refused = await request.put(`/api/focus/${moving.id}`, { data: {} });
  expect(refused.status()).toBe(422);
  expect(await refused.text()).toContain('parked');

  // Active again: back in its place, with its why-line.
  await declare(request, moving.id, 'active');
  const back = await (await request.get('/api/focus')).json();
  expect(back.map((e: { project: { title: string } }) => e.project.title)).toEqual([
    kept.title,
    moving.title,
  ]);
  expect(back[1].focus.why_now).toBe('needs me this week');

  // Complete: it leaves Focus, and re-opening it does not put it back.
  await declare(request, moving.id, 'complete');
  expect(await focusTitles(request)).toEqual([kept.title]);
  const closed = await request.put(`/api/focus/${moving.id}`, { data: {} });
  expect(closed.status()).toBe(422);
  expect(await closed.text()).toContain('closed');
  await declare(request, moving.id, 'active');
  expect(await focusTitles(request)).toEqual([kept.title]);
});

test('the dashboard and the Projects list follow the state', async ({ request }) => {
  const stamp = Date.now();
  const active = await project(request, `${PREFIX}active ${stamp}`);
  const parked = await project(request, `${PREFIX}parked ${stamp}`);
  const done = await project(request, `${PREFIX}done ${stamp}`);
  const dropped = await project(request, `${PREFIX}dropped ${stamp}`);
  await declare(request, parked.id, 'parked');
  await declare(request, done.id, 'complete');
  await declare(request, dropped.id, 'cancelled');

  // The dashboard: active and parked projects, each with its state. A
  // complete or cancelled one has left it.
  const cards: { node_id: string; project_state: string; project_state_at: string | null }[] =
    await (await request.get('/api/dashboard/projects')).json();
  const stateOf = (id: string) => cards.find((c) => c.node_id === id)?.project_state;
  expect(stateOf(active.id)).toBe('active');
  expect(stateOf(parked.id)).toBe('parked');
  expect(cards.find((c) => c.node_id === parked.id)?.project_state_at).toBeTruthy();
  expect(stateOf(done.id)).toBeUndefined();
  expect(stateOf(dropped.id)).toBeUndefined();

  // The Projects list filters on the state, server side.
  const listed = async (state: string): Promise<string[]> => {
    const resp = await request.get(
      `/api/nodes?node_type=project&project_state=${state}&per_page=200`,
    );
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    return body.nodes.map((n: { id: string }) => n.id);
  };
  const mine = [active.id, parked.id, done.id, dropped.id];
  for (const [state, id] of [
    ['active', active.id],
    ['parked', parked.id],
    ['complete', done.id],
    ['cancelled', dropped.id],
  ] as const) {
    const ids = await listed(state);
    expect(
      mine.filter((m) => ids.includes(m)),
      state,
    ).toEqual([id]);
  }
});

test('Data health lists a label that reads as another state until one is declared', async ({
  request,
}) => {
  const stamp = Date.now();
  const rule = 'label_reads_as_another_state';
  type Item = {
    subject: { id: string };
    label_reading?: { label: string; reads_as: string; state: string };
  };
  const items = async (): Promise<Item[]> => {
    const report = await (await request.get('/api/data-health')).json();
    return report.rules.find((r: { rule: string }) => r.rule === rule)?.items ?? [];
  };
  const onHold = await project(request, `${PREFIX}on hold ${stamp}`, 'On hold');
  const plain = await project(request, `${PREFIX}plain ${stamp}`, 'Active');

  const found = (await items()).find((i) => i.subject.id === onHold.id);
  expect(found?.label_reading).toEqual({ label: 'On hold', reads_as: 'parked', state: 'active' });
  expect((await items()).some((i) => i.subject.id === plain.id)).toBe(false);

  // Keeping the state it has is a declaration too: the item is settled.
  await declare(request, onHold.id, 'active');
  expect((await items()).some((i) => i.subject.id === onHold.id)).toBe(false);
});

// ── In the app (S6a, PR 2) ───────────────────────────────────────────────────

async function addTask(request: APIRequestContext, nodeId: string, data: Record<string, unknown>) {
  const resp = await request.post(`/api/nodes/${nodeId}/tasks`, { data });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

const stateOf = async (request: APIRequestContext, id: string): Promise<string> =>
  (await (await request.get(`/api/nodes/${id}`)).json()).project_state;

test('a project is parked, made active and completed from its page', async ({ page, request }) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}page ${stamp}`);
  const article = await createNode(request, `${PREFIX}page article ${stamp}`);

  // Only a Project has the control.
  await page.goto(`/nodes/${article.id}`);
  await expect(page.getByTestId('node-notes')).toBeVisible();
  await expect(page.getByTestId('project-state-chip')).toHaveCount(0);

  await page.goto(`/nodes/${p.id}`);
  const chip = page.getByTestId('project-state-chip');
  const option = (state: string) => page.getByTestId(`project-state-option-${state}`);
  const focus = page.getByTestId('focus-toggle');
  // Active is the unmarked case: the icon alone, named for a screen reader.
  await expect(chip).toHaveAttribute('data-state', 'active');
  await expect(chip).toHaveAccessibleName('State: Active. Change state');
  await expect(focus).toHaveAccessibleName('Add to Focus');
  await focus.click();
  await expect(focus).toHaveAccessibleName('Remove from Focus');

  // Park it: the chip says so and since when, and Focus has no control
  // for a project that is set aside.
  await chip.click();
  await expect(option('active')).toHaveAttribute('aria-checked', 'true');
  await option('parked').click();
  await expect(chip).toHaveAttribute('data-state', 'parked');
  await expect(chip).toContainText(/Parked since [A-Z][a-z]{2} \d/);
  await expect(focus).toHaveCount(0);
  await expect.poll(() => stateOf(request, p.id)).toBe('parked');

  // Escape closes the choices without changing anything, and stays here.
  // The keyboard goes to the state it has, and comes back to the chip.
  await chip.click();
  await expect(option('parked')).toHaveAttribute('aria-checked', 'true');
  await expect(option('parked')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('project-state-options')).toHaveCount(0);
  await expect(chip).toBeFocused();
  expect(new URL(page.url()).pathname).toBe(`/nodes/${p.id}`);
  // And by keyboard alone: open, move to Active, choose it.
  await page.keyboard.press('Enter');
  await expect(option('parked')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(option('active')).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(chip).toHaveAttribute('data-state', 'active');
  await chip.click();
  await option('parked').click();
  await expect(chip).toHaveAttribute('data-state', 'parked');

  // Active again: it is back in Focus as it was.
  await chip.click();
  await option('active').click();
  await expect(chip).toHaveAttribute('data-state', 'active');
  await expect(focus).toHaveAccessibleName('Remove from Focus');

  // Complete: it has left Focus for good, and History has each change.
  await chip.click();
  await option('complete').click();
  await expect(chip).toContainText(/Complete since/);
  await expect(focus).toHaveCount(0);
  await expect.poll(() => stateOf(request, p.id)).toBe('complete');
  await chip.click();
  await option('active').click();
  await expect(focus).toHaveAccessibleName('Add to Focus');

  const section = page.getByTestId('node-activity');
  const toggle = section.locator('button[aria-expanded]').first();
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
  // One line per change: parked twice above (once by mouse, once after the
  // keyboard made it active), completed and re-opened once.
  await expect(section.getByText('Active → Parked')).toHaveCount(2);
  await expect(section.getByText('Active → Complete')).toHaveCount(1);
  await expect(section.getByText('Complete → Active')).toHaveCount(1);
});

test('a failed save puts the state chip back and says so', async ({ page, request }) => {
  const p = await project(request, `${PREFIX}failing ${Date.now()}`);
  await page.goto(`/nodes/${p.id}`);
  const chip = page.getByTestId('project-state-chip');
  await expect(chip).toHaveAttribute('data-state', 'active');
  await page.route('**/api/nodes/*/project-state', (route) =>
    route.fulfill({ status: 500, json: { error: 'no' } }),
  );
  await chip.click();
  await page.getByTestId('project-state-option-parked').click();
  await expect(page.locator('[role="alert"], [role="status"]').filter({ hasText: /save the state/ })).toBeVisible();
  await expect(chip).toHaveAttribute('data-state', 'active');
  expect(await stateOf(request, p.id)).toBe('active');
});

test('a viewer reads the state and has no control to change it', async ({
  page,
  request,
  otherRequest,
}) => {
  const p = await project(otherRequest, `${PREFIX}theirs ${Date.now()}`);
  try {
    await shareNode(otherRequest, p.id, 'e2e-test-user', 'viewer');
    await declare(otherRequest, p.id, 'parked');
    await page.goto(`/nodes/${p.id}`);
    await expect(page.getByTestId('project-state').getByTestId('project-state-badge')).toContainText(
      /Parked since/,
    );
    await expect(page.getByTestId('project-state-chip')).toHaveCount(0);
    await expect(page.getByTestId('focus-toggle')).toHaveCount(0);
  } finally {
    await otherRequest.delete(`/api/nodes/${p.id}`);
  }
});

test('the dashboard keeps parked projects in a closed group', async ({ page, request }) => {
  const stamp = Date.now();
  const moving = await project(request, `${PREFIX}card moving ${stamp}`);
  const set_aside = await project(request, `${PREFIX}card aside ${stamp}`);
  const done = await project(request, `${PREFIX}card done ${stamp}`);
  await declare(request, set_aside.id, 'parked');
  await declare(request, done.id, 'complete');

  await page.goto('/dashboard');
  const groups = page.getByTestId('project-groups');
  const parked = page.getByTestId('parked-group');
  const toggle = page.getByTestId('parked-toggle');
  const card = (title: string) => groups.getByTestId('project-card').filter({ hasText: title });
  await expect(card(moving.title)).toBeVisible();
  // Closed until asked for, and a complete project is not here at all.
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(card(set_aside.title)).toHaveCount(0);
  await expect(card(done.title)).toHaveCount(0);

  await toggle.click();
  const aside = parked.getByTestId('project-card').filter({ hasText: set_aside.title });
  await expect(aside).toBeVisible();
  await expect(aside.getByTestId('project-state-badge')).toContainText(/Parked since/);
  // A parked project can't be in Focus: its card has no toggle.
  await expect(aside.getByTestId('focus-toggle')).toHaveCount(0);

  // Make it active from its card: it joins the Area groups.
  await aside.getByRole('button', { name: 'Make active' }).click();
  await expect(parked.getByTestId('project-card').filter({ hasText: set_aside.title })).toHaveCount(0);
  const active = card(set_aside.title).and(page.locator('[data-project-state="active"]'));
  await expect(active).toBeVisible();
  expect(await stateOf(request, set_aside.id)).toBe('active');

  // Park the other from its card. The click doesn't open the project.
  await card(moving.title)
    .and(page.locator('[data-project-state="active"]'))
    .getByRole('button', { name: /^Park/ })
    .click();
  await expect(parked.getByTestId('project-card').filter({ hasText: moving.title })).toBeVisible();
  expect(new URL(page.url()).pathname).toBe('/dashboard');
  expect(await stateOf(request, moving.id)).toBe('parked');

  // The group was left open: it is open on the next visit to this device.
  await page.reload();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
});

test('the Projects list marks a parked project and filters by state', async ({ page, request }) => {
  const stamp = Date.now();
  const active = await project(request, `${PREFIX}list active ${stamp}`);
  const set_aside = await project(request, `${PREFIX}list aside ${stamp}`);
  await declare(request, set_aside.id, 'parked');

  await page.goto('/nodes');
  await expect(page.locator('main')).toBeVisible();
  await page.locator('button[title="Projects"]').first().click();
  const row = (title: string) => page.locator('li', { hasText: title });
  await expect(row(set_aside.title).getByTestId('project-state-badge')).toHaveText(/Parked/);
  // Active is the unmarked case.
  await expect(row(active.title)).toBeVisible();
  await expect(row(active.title).getByTestId('project-state-badge')).toHaveCount(0);

  const filter = page.getByTestId('project-state-filter');
  await filter.getByRole('button', { name: 'State: Any' }).click();
  await page.getByRole('menu', { name: 'Project state' }).getByRole('menuitem', { name: 'Parked' }).click();
  await expect(row(set_aside.title)).toBeVisible();
  await expect(row(active.title)).toHaveCount(0);
  await expect(filter.getByRole('button', { name: 'State: Parked' })).toBeVisible();
});

test("a parked project's tasks leave Today and Overdue, and stay in the Backlog", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}my day ${stamp}`);
  const yesterday = (() => {
    const d = new Date();
    d.setDate(d.getDate() - 1);
    return d;
  })();
  const iso = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const earlier = new Date(yesterday);
  earlier.setDate(earlier.getDate() - 2);
  const carried = await addTask(request, p.id, { title: `carried ${stamp}`, focus_date: iso(yesterday) });
  const late = await addTask(request, p.id, { title: `late ${stamp}`, due_date: iso(earlier) });
  const chosen = await addTask(request, p.id, { title: `chosen today ${stamp}`, focus_date: iso(new Date()) });

  await page.goto('/tasks/my-day');
  const today = page.getByTestId('zone-today');
  const overdue = page.getByTestId('overdue-section');
  const backlog = page.getByTestId('zone-backlog');
  const rowIn = (zone: typeof today, id: string) => zone.locator(`[data-task-id="${id}"]`);
  // While the project is active: the normal flow.
  await expect(rowIn(today, carried.id)).toBeVisible();
  await expect(rowIn(overdue, late.id)).toBeVisible();

  await declare(request, p.id, 'parked');
  await page.reload();
  // What its person put in today's list stays: they chose it.
  await expect(rowIn(today, chosen.id)).toBeVisible();
  await expect(rowIn(today, carried.id)).toHaveCount(0);
  await expect(rowIn(overdue, late.id)).toHaveCount(0);
  // Neither is lost: both are in the Backlog, each saying why.
  for (const id of [carried.id, late.id]) {
    const row = rowIn(backlog, id);
    await expect(row.getByTestId('task-project-state')).toHaveText('Project parked');
    await expect(row.getByTestId('carryover-prompt')).toHaveCount(0);
  }

  // Active again: back in the normal flow.
  await declare(request, p.id, 'active');
  await page.reload();
  await expect(rowIn(today, carried.id)).toBeVisible();
  await expect(rowIn(overdue, late.id)).toBeVisible();
  await expect(rowIn(today, carried.id).getByTestId('task-project-state')).toHaveCount(0);
});

test('Data health offers to park, or to keep the state, for a label that reads otherwise', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const onHold = await project(request, `${PREFIX}dh on hold ${stamp}`, 'On hold');
  const kept = await project(request, `${PREFIX}dh kept ${stamp}`, 'Paused');

  await page.goto('/data-health');
  const card = page.getByTestId('data-health-label_reads_as_another_state');
  const item = (title: string) => card.getByTestId('data-health-item').filter({ hasText: title });
  await expect(item(onHold.title).getByTestId('label-reading')).toHaveText(
    '“On hold” reads as parked; the project is active',
  );

  // The state the label means.
  await item(onHold.title).getByRole('button', { name: 'Park' }).click();
  await expect(item(onHold.title)).toHaveCount(0);
  expect(await stateOf(request, onHold.id)).toBe('parked');

  // Or the state it has: the label stays, and the item is settled.
  await item(kept.title).getByRole('button', { name: 'Keep active' }).click();
  await expect(item(kept.title)).toHaveCount(0);
  const node = await (await request.get(`/api/nodes/${kept.id}`)).json();
  expect(node.project_state).toBe('active');
  expect(node.project_state_at).toBeTruthy();
  expect(node.project_status).toBe('Paused');
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 800 } });

  test('the state choices fit the screen', async ({ page, request }) => {
    const p = await project(request, `${PREFIX}phone ${Date.now()}`);
    await declare(request, p.id, 'parked');
    await page.goto(`/nodes/${p.id}`);
    const chip = page.getByTestId('project-state-chip');
    await expect(chip).toContainText(/Parked since/);
    await chip.click();
    const options = page.getByTestId('project-state-options');
    await expect(options).toBeVisible();
    const box = await options.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(375);
    // And the page itself doesn't scroll sideways with them open.
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
  });
});
