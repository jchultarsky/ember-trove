import {
  test,
  expect,
  createNode,
  createProject,
  addTask,
  deleteNodesByPrefix,
  shareNode,
  E2E_SUBJECT,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * Blockers (S7b, #346; ONTOLOGY §6.4, §9.3, §14): a node that depends on
 * something not finished says "Blocked", "Depends on" says what is met,
 * and what those wait for in turn is listed with them. Display only:
 * nothing is prevented or moved. A reader is shown only what they can
 * open.
 *
 * Every title starts with PREFIX; nodes are deleted by id as well (a
 * closed or archived one is not in the lookup the prefix wipe uses).
 */

const PREFIX = 'e2e blk ';
const OTHER_SUBJECT = 'e2e-other-user';

const wipe = (request: APIRequestContext) => deleteNodesByPrefix(request, PREFIX);

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});

type Made = { id: string; title: string };

async function dependsOn(request: APIRequestContext, source: Made, target: Made) {
  const made = await request.post('/api/edges', {
    data: { source_id: source.id, target_id: target.id, edge_type: 'depends_on' },
  });
  expect(made.status(), await made.text()).toBe(201);
}

async function declare(request: APIRequestContext, project: Made, state: string) {
  const saved = await request.put(`/api/nodes/${project.id}/project-state`, { data: { state } });
  expect(saved.ok(), await saved.text()).toBeTruthy();
}

async function setStatus(request: APIRequestContext, node: Made, status: string) {
  const saved = await request.put(`/api/nodes/${node.id}`, { data: { status } });
  expect(saved.ok()).toBeTruthy();
}

/** Counts the reads of a node's chain the page makes from here on. */
async function countChainReads(page: Page, id: string) {
  const reads = { count: 0 };
  await page.route(`**/api/nodes/${id}/blockers`, (route) => {
    reads.count += 1;
    return route.fallback();
  });
  return reads;
}

const chip = (page: Page) => page.getByTestId('node-blocked');
const group = (page: Page, slug: string) => page.getByTestId(`connections-${slug}`);
const rows = (page: Page, slug: string) => group(page, slug).getByTestId('connection-row');
const chain = (page: Page) => page.getByTestId('connections-chain');

/** Opens a node's page and waits for its sections. */
async function openNode(page: Page, id: string) {
  await page.goto(`/nodes/${id}`);
  await expect(page.getByTestId('node-connections')).toBeVisible({ timeout: 15_000 });
}

test('a node that waits for something unfinished says Blocked, and says what', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const make = (name: string, node_type: string) =>
    createNode(request, `${PREFIX}${name} ${stamp}`, { node_type });
  const waiting = await make('the waiting one', 'project');
  const running = await make('a running project', 'project');
  const draft = await make('a draft behind it', 'article');
  const manual = await make('the manual', 'reference');
  const stopped = await make('z stopped project', 'project');
  const made = [waiting, running, draft, manual, stopped];
  try {
    await dependsOn(request, waiting, running);
    await dependsOn(request, running, draft);
    await dependsOn(request, waiting, manual);
    await dependsOn(request, waiting, stopped);
    await declare(request, stopped, 'cancelled');

    // On a fresh load, with Connections still closed, and without a read
    // of the chain: the node's own answer carries the count.
    const reads = await countChainReads(page, waiting.id);
    await openNode(page, waiting.id);
    await expect(chip(page)).toBeVisible();
    await expect(chip(page)).toHaveAttribute('title', 'Waits for 2 nodes that are not finished');
    await expect(page.getByTestId('connections-groups')).toHaveCount(0);
    expect(reads.count).toBe(0);

    // The chip opens Connections. "Depends on" lists what is not met
    // first (a cancelled Project never lets it go: it must not sink to
    // the bottom), and every row says "Met" or what the other node is.
    await chip(page).click();
    await expect(rows(page, 'depends-on')).toHaveText([
      new RegExp(`${running.title}.*Active`),
      new RegExp(`${stopped.title}.*Cancelled`),
      new RegExp(`${manual.title}.*Met`),
    ]);
    // What those wait for in turn, and through which of this node's own
    // dependencies. Not this node's link: no remove control.
    await expect(chain(page).getByRole('heading')).toContainText('Further down the chain');
    const further = chain(page).getByTestId('chain-row');
    await expect(further).toHaveText([new RegExp(`${draft.title}.*Draft.*through ${running.title}`)]);
    await expect(chain(page).getByRole('button')).toHaveCount(0);
    expect(reads.count).toBe(1);

    // Removing a dependency here shows in the chip at once.
    await group(page, 'depends-on')
      .getByRole('button', { name: `Remove the link to ${stopped.title}` })
      .click();
    await expect(rows(page, 'depends-on')).toHaveCount(2);
    await expect(chip(page)).toHaveAttribute('title', 'Waits for 1 node that is not finished');

    // Once what it waited for is complete, nothing says "Blocked", the
    // row says "Met", and there is no chain (what a finished thing
    // waited for no longer matters) and no read of one.
    await declare(request, running, 'complete');
    const again = await countChainReads(page, waiting.id);
    await openNode(page, waiting.id);
    await expect(chip(page)).toHaveCount(0);
    await page.getByTestId('node-connections').getByRole('button', { name: /Connections/ }).click();
    await expect(rows(page, 'depends-on')).toHaveText([
      new RegExp(`${running.title}.*Met`),
      new RegExp(`${manual.title}.*Met`),
    ]);
    await expect(chain(page)).toHaveCount(0);
    await page.waitForLoadState('networkidle');
    expect(again.count).toBe(0);
  } finally {
    for (const n of made) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('a node that is finished, or out of use, does not say Blocked', async ({ page, request }) => {
  const stamp = Date.now();
  const running = await createNode(request, `${PREFIX}still running ${stamp}`, {
    node_type: 'project',
  });
  const essay = await createNode(request, `${PREFIX}an essay ${stamp}`);
  const closing = await createNode(request, `${PREFIX}a closing project ${stamp}`, {
    node_type: 'project',
  });
  try {
    await dependsOn(request, essay, running);
    await dependsOn(request, closing, running);

    // A draft Article still waits.
    await openNode(page, essay.id);
    await expect(chip(page)).toBeVisible();
    // Published, it is finished: what depends on it reads it as met, so
    // it can't say it is still waiting.
    await setStatus(request, essay, 'published');
    await openNode(page, essay.id);
    await expect(page.getByTestId('node-history')).toBeVisible();
    await expect(chip(page)).toHaveCount(0);

    // A Project says so while it is active, and no longer once closed.
    await openNode(page, closing.id);
    await expect(chip(page)).toBeVisible();
    await declare(request, closing, 'cancelled');
    await openNode(page, closing.id);
    await expect(page.getByTestId('node-history')).toBeVisible();
    await expect(chip(page)).toHaveCount(0);
  } finally {
    for (const n of [running, essay, closing]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('a chain stops at a node its reader cannot see', async ({ page, request, otherRequest }) => {
  const stamp = Date.now();
  const mine = await createNode(request, `${PREFIX}my project ${stamp}`, { node_type: 'project' });
  const theirs = await createNode(otherRequest, `${PREFIX}their shared one ${stamp}`, {
    node_type: 'project',
  });
  const hidden = await createNode(otherRequest, `${PREFIX}their private one ${stamp}`, {
    node_type: 'project',
  });
  const beyond = await createNode(request, `${PREFIX}mine again ${stamp}`, { node_type: 'project' });
  try {
    await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'viewer');
    await shareNode(request, mine.id, OTHER_SUBJECT, 'viewer');
    await shareNode(request, beyond.id, OTHER_SUBJECT, 'viewer');
    await dependsOn(request, mine, theirs);
    await dependsOn(otherRequest, theirs, hidden);
    await dependsOn(otherRequest, hidden, beyond);

    // I see my own dependency, and nothing of what it waits for: the
    // hidden node, and my own node behind it, read as "nothing there".
    await openNode(page, mine.id);
    await chip(page).click();
    await expect(rows(page, 'depends-on')).toHaveText([new RegExp(`${theirs.title}.*Active`)]);
    await page.waitForLoadState('networkidle');
    await expect(chain(page)).toHaveCount(0);
    await expect(page.getByTestId('node-connections')).not.toContainText(hidden.title);
    const seen = await (await request.get(`/api/nodes/${mine.id}/blockers`)).json();
    expect(seen).toEqual({ also: [], truncated: false });

    // Its owner, who can see all four, gets the whole chain.
    const whole = await (await otherRequest.get(`/api/nodes/${mine.id}/blockers`)).json();
    expect(whole.also.map((b: { title: string }) => b.title)).toEqual([hidden.title, beyond.title]);

    // The count on each node is the reader's too.
    const node = (r: APIRequestContext, id: string) =>
      r.get(`/api/nodes/${id}`).then((resp) => resp.json());
    expect((await node(request, theirs.id)).blocked_by).toBe(0);
    expect((await node(otherRequest, theirs.id)).blocked_by).toBe(1);
  } finally {
    for (const n of [mine, beyond]) await request.delete(`/api/nodes/${n.id}`);
    for (const n of [theirs, hidden]) await otherRequest.delete(`/api/nodes/${n.id}`);
  }
});

test('the dashboard card says Blocked and counts its blocked tasks', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const blocked = await createProject(request, `${PREFIX}b held up ${stamp}`);
  const free = await createProject(request, `${PREFIX}a free ${stamp}`);
  const needed = await createProject(request, `${PREFIX}c needed first ${stamp}`);
  try {
    await dependsOn(request, blocked, needed);
    const paint = await addTask(request, blocked.id, { title: `paint ${stamp}` });
    const buy = await addTask(request, blocked.id, { title: `buy paint ${stamp}` });
    const also = await addTask(request, blocked.id, { title: `sand ${stamp}` });
    for (const waits of [paint, also]) {
      const added = await request.post(`/api/tasks/${waits.id}/dependencies`, {
        data: { depends_on: buy.id },
      });
      expect(added.status()).toBe(201);
    }

    await page.goto('/dashboard');
    const cards = page.getByTestId('project-groups').getByTestId('project-card');
    const card = (p: Made) => cards.filter({ hasText: p.title });
    await expect(card(blocked).getByTestId('project-blocked')).toBeVisible();
    await expect(card(blocked).getByTestId('project-blocked')).toHaveAttribute(
      'title',
      'Waits for 1 node that is not finished',
    );
    const tasks = card(blocked).getByTestId('count-blocked');
    await expect(tasks).toHaveText(/2/);
    await expect(tasks).toHaveAttribute('title', '2 of its tasks wait for another task');
    // A Project that waits for nothing says nothing.
    await expect(card(free).getByTestId('project-blocked')).toHaveCount(0);
    await expect(card(free).getByTestId('count-blocked')).toHaveCount(0);
    await expect(card(needed).getByTestId('project-blocked')).toHaveCount(0);

    // Surfaced, never enforced: being blocked, or freed, moves no card.
    const order = async () =>
      (await cards.allTextContents())
        .map((text) => [blocked, free, needed].find((p) => text.includes(p.title))?.title)
        .filter(Boolean);
    const before = await order();
    expect(before).toHaveLength(3);
    // Finished, the prerequisite frees the tasks and the Project.
    await request.patch(`/api/tasks/${buy.id}`, { data: { status: 'done' } });
    await declare(request, needed, 'complete');
    await page.reload();
    await expect(card(blocked)).toBeVisible();
    await expect(card(blocked).getByTestId('project-blocked')).toHaveCount(0);
    await expect(card(blocked).getByTestId('count-blocked')).toHaveCount(0);
    // The two that are still listed stand in the order they had.
    expect(await order()).toEqual(before.filter((title) => title !== needed.title));
  } finally {
    for (const n of [blocked, free, needed]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('the task editor lists what a task waits for further down the chain', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = await createProject(request, `${PREFIX}plan ${stamp}`);
  try {
    const paint = await addTask(request, project.id, { title: `paint ${stamp}` });
    const buy = await addTask(request, project.id, { title: `buy paint ${stamp}` });
    const choose = await addTask(request, project.id, { title: `choose a colour ${stamp}` });
    for (const [waits, on] of [
      [paint, buy],
      [buy, choose],
    ]) {
      const added = await request.post(`/api/tasks/${waits.id}/dependencies`, {
        data: { depends_on: on.id },
      });
      expect(added.status()).toBe(201);
    }

    await page.goto(`/nodes/${project.id}`);
    const row = page.locator(`[data-task-id="${paint.id}"]`);
    await row.getByRole('button', { name: 'Edit task' }).click();
    const section = row.getByTestId('task-dependencies');
    await expect(section.getByTestId('blocked-by-list')).toContainText(buy.title);
    // Not its own link: listed with the prerequisite it comes through,
    // and with no remove control.
    const further = section.getByTestId('chain-list').getByTestId('dependency');
    await expect(further).toHaveText([new RegExp(`${choose.title}.*through ${buy.title}`)]);
    await expect(section.getByTestId('chain-list').getByRole('button')).toHaveCount(0);

    // Once the middle task is done, nothing is further down.
    await request.patch(`/api/tasks/${buy.id}`, { data: { status: 'done' } });
    await page.reload();
    await page
      .locator(`[data-task-id="${paint.id}"]`)
      .getByRole('button', { name: 'Edit task' })
      .click();
    const after = page.locator(`[data-task-id="${paint.id}"]`).getByTestId('task-dependencies');
    await expect(after.getByTestId('blocked-by-list')).toContainText(buy.title);
    await expect(after.getByTestId('chain-list')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the header with its Blocked chip fits the screen', async ({ page, request }) => {
    const stamp = Date.now();
    const waiting = await createNode(
      request,
      `${PREFIX}cabinets, counters and the long-delayed lighting plan ${stamp}`,
      { node_type: 'project' },
    );
    const needed = await createNode(request, `${PREFIX}the electrician ${stamp}`, {
      node_type: 'project',
    });
    try {
      await dependsOn(request, waiting, needed);
      await openNode(page, waiting.id);
      await expect(chip(page)).toBeVisible();
      await page.waitForLoadState('networkidle');
      const main = page.locator('main');
      const over = await main.evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(over, `the page is ${over}px wider than the screen`).toBeLessThanOrEqual(1);
      // Open, with the chain's row.
      await chip(page).click();
      await expect(rows(page, 'depends-on')).toHaveCount(1);
      const wide = await main.evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(wide, `Connections is ${wide}px wider than the screen`).toBeLessThanOrEqual(1);
    } finally {
      for (const n of [waiting, needed]) await request.delete(`/api/nodes/${n.id}`);
    }
  });
});
