import {
  test,
  expect,
  createNode,
  deleteNodesByPrefix,
  shareNode,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * Links not shown (#349; ONTOLOGY §14, §6.2, §6.4). Someone a node is
 * shared with can link it to a node of theirs that its owner can't see.
 * The owner is shown no such link, and it still counts for loops and for
 * "Blocked". Whoever may edit the node is told how many there are, never
 * which, and removes them together. A viewer is told nothing, and nobody
 * is told of the owner's own private links.
 *
 * Every title starts with PREFIX; nodes are deleted by id, and the
 * prefix wipe runs before and after each test for what a failed attempt
 * left.
 */

const PREFIX = 'e2e hid ';
const OTHER_SUBJECT = 'e2e-other-user';

const wipe = (request: APIRequestContext) => deleteNodesByPrefix(request, PREFIX);

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});
test.afterEach(async ({ page, request, otherRequest }) => {
  await page.setExtraHTTPHeaders({});
  await wipe(request);
  await wipe(otherRequest);
});

type Made = { id: string; title: string };

async function link(request: APIRequestContext, source: Made, target: Made, edge_type: string) {
  const made = await request.post('/api/edges', {
    data: { source_id: source.id, target_id: target.id, edge_type },
  });
  expect(made.status(), `${edge_type}: ${await made.text()}`).toBe(201);
}

/** The node's own answer, as the caller is given it. */
async function told(request: APIRequestContext, node: Made) {
  const resp = await request.get(`/api/nodes/${node.id}`);
  expect(resp.ok()).toBeTruthy();
  return (await resp.json()) as { links_not_shown: number; blocked_by: number };
}

/** The page runs as the second identity from the next load on. */
const asOther = (page: Page) => page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
const asMe = (page: Page) => page.setExtraHTTPHeaders({});

async function openNode(page: Page, id: string) {
  await page.goto(`/nodes/${id}`);
  await expect(page.getByTestId('node-connections')).toBeVisible({ timeout: 15_000 });
}

async function openConnections(page: Page) {
  await page
    .getByTestId('node-connections')
    .getByRole('button', { name: /Connections/ })
    .click();
}

const marker = (page: Page) => page.getByTestId('node-links-not-shown');
const line = (page: Page) => page.getByTestId('links-not-shown');
const blocked = (page: Page) => page.getByTestId('node-blocked');
const rows = (page: Page) => page.getByTestId('connection-row');

test('the owner is told of a link an editor made to a node she can’t see, and removes it', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const mine = await createNode(request, `${PREFIX}my project ${stamp}`, { node_type: 'project' });
  const source = await createNode(request, `${PREFIX}my reference ${stamp}`, {
    node_type: 'reference',
  });
  const theirs = await createNode(otherRequest, `${PREFIX}their private project ${stamp}`, {
    node_type: 'project',
  });
  try {
    await shareNode(request, mine.id, OTHER_SUBJECT, 'editor');
    await link(request, mine, source, 'references');
    // The editor makes my project depend on a project of theirs.
    await link(otherRequest, mine, theirs, 'depends_on');

    // I am told one link isn't shown, and nothing about what it is. They
    // see it, and read my project as blocked.
    expect(await told(request, mine)).toMatchObject({ links_not_shown: 1, blocked_by: 0 });
    expect(await told(otherRequest, mine)).toMatchObject({ links_not_shown: 0, blocked_by: 1 });
    await asOther(page);
    await openNode(page, mine.id);
    await expect(blocked(page)).toBeVisible();
    await expect(marker(page)).toHaveCount(0);

    // My page: the marker in the header, before Connections is opened;
    // not "Blocked", which would say what the link is.
    await asMe(page);
    await openNode(page, mine.id);
    await expect(marker(page)).toHaveText(/1 link not shown/);
    await expect(blocked(page)).toHaveCount(0);
    await expect(line(page)).toHaveCount(0);

    // It opens Connections: the line, the link I can see, no title of
    // theirs.
    await marker(page).click();
    await expect(line(page)).toContainText('1 link of this node isn’t shown here');
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText(source.title);
    await expect(page.getByText(theirs.title)).toHaveCount(0);

    // The remove asks first; Cancel leaves everything.
    const remove = line(page).getByRole('button', { name: 'Remove the link not shown' });
    await remove.click();
    const confirm = page.getByRole('group', { name: 'Confirm remove' });
    await expect(confirm).toContainText('Remove the link you aren’t shown?');
    await confirm.getByRole('button', { name: 'Cancel' }).click();
    await expect(line(page)).toContainText('1 link of this node isn’t shown here');
    expect((await told(request, mine)).links_not_shown).toBe(1);

    await remove.click();
    await confirm.getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(page.getByText('Removed 1 link')).toBeVisible();
    await expect(marker(page)).toHaveCount(0);
    await expect(line(page)).toHaveCount(0);
    // The link I could see is untouched.
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText(source.title);

    // It is gone for them too: my project no longer reads as blocked.
    expect(await told(request, mine)).toMatchObject({ links_not_shown: 0 });
    expect(await told(otherRequest, mine)).toMatchObject({ links_not_shown: 0, blocked_by: 0 });
    await asOther(page);
    await openNode(page, mine.id);
    await expect(blocked(page)).toHaveCount(0);
  } finally {
    await page.setExtraHTTPHeaders({});
    for (const n of [mine, source]) await request.delete(`/api/nodes/${n.id}`);
    await otherRequest.delete(`/api/nodes/${theirs.id}`);
  }
});

test('a viewer is told nothing, and an editor nothing about the owner’s own private links', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const mine = await createNode(request, `${PREFIX}shared project ${stamp}`, {
    node_type: 'project',
  });
  const secret = await createNode(request, `${PREFIX}my private reference ${stamp}`, {
    node_type: 'reference',
  });
  const theirs = await createNode(otherRequest, `${PREFIX}their private article ${stamp}`, {
    node_type: 'article',
  });
  const clear = (r: APIRequestContext) => r.delete(`/api/nodes/${mine.id}/edges/not-shown`);
  try {
    await shareNode(request, mine.id, OTHER_SUBJECT);
    await link(request, mine, secret, 'references');
    // Someone who may only view my project relates a private node of
    // theirs to it.
    await link(otherRequest, theirs, mine, 'related_to');

    // I am told of that one. The viewer is told nothing, and may not
    // clear anything.
    expect((await told(request, mine)).links_not_shown).toBe(1);
    expect((await told(otherRequest, mine)).links_not_shown).toBe(0);
    expect((await clear(otherRequest)).status()).toBe(403);
    await asOther(page);
    await openNode(page, mine.id);
    await expect(marker(page)).toHaveCount(0);
    await openConnections(page);
    // They are shown their own link, and nothing of my private one.
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText(theirs.title);
    await expect(line(page)).toHaveCount(0);
    await expect(page.getByText(secret.title)).toHaveCount(0);

    // As an editor they are still told nothing: the link to my private
    // reference is one I can see. Their remove takes nothing.
    await shareNode(request, mine.id, OTHER_SUBJECT, 'editor');
    expect((await told(otherRequest, mine)).links_not_shown).toBe(0);
    const cleared = await clear(otherRequest);
    expect(cleared.status()).toBe(200);
    expect(await cleared.json()).toEqual({ removed: 0, links_not_shown: 0 });
    const still: { target_id: string }[] = await (
      await request.get(`/api/nodes/${mine.id}/edges`)
    ).json();
    expect(still.map((e) => e.target_id)).toEqual([secret.id]);
    await openNode(page, mine.id);
    await expect(marker(page)).toHaveCount(0);
    await openConnections(page);
    await expect(rows(page)).toHaveCount(1);
    await expect(line(page)).toHaveCount(0);
    await expect(page.getByText(secret.title)).toHaveCount(0);

    // And I still am: their link into my project.
    await asMe(page);
    await openNode(page, mine.id);
    await expect(marker(page)).toHaveText(/1 link not shown/);
  } finally {
    await page.setExtraHTTPHeaders({});
    for (const n of [mine, secret]) await request.delete(`/api/nodes/${n.id}`);
    await otherRequest.delete(`/api/nodes/${theirs.id}`);
  }
});

test('the line shows on a node whose only link is one not shown', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const mine = await createNode(request, `${PREFIX}lone project ${stamp}`, { node_type: 'project' });
  const theirs = await createNode(otherRequest, `${PREFIX}their own note ${stamp}`, {
    node_type: 'article',
  });
  try {
    await shareNode(request, mine.id, OTHER_SUBJECT);
    await link(otherRequest, theirs, mine, 'related_to');

    // Nothing to list, and still something to say: beside the empty
    // state, not instead of it.
    await openNode(page, mine.id);
    await marker(page).click();
    await expect(page.getByTestId('connections-empty')).toBeVisible();
    await expect(line(page)).toContainText('1 link of this node isn’t shown here');

    await line(page).getByRole('button', { name: 'Remove the link not shown' }).click();
    await page
      .getByRole('group', { name: 'Confirm remove' })
      .getByRole('button', { name: 'Remove', exact: true })
      .click();
    await expect(line(page)).toHaveCount(0);
    await expect(marker(page)).toHaveCount(0);
    await expect(page.getByTestId('connections-empty')).toBeVisible();
    // Their own page no longer lists it either.
    const left: unknown[] = await (await otherRequest.get(`/api/nodes/${theirs.id}/edges`)).json();
    expect(left).toEqual([]);
  } finally {
    await request.delete(`/api/nodes/${mine.id}`);
    await otherRequest.delete(`/api/nodes/${theirs.id}`);
  }
});
