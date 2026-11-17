import {
  test,
  expect,
  createNode,
  deleteNodesByPrefix,
  shareNode,
  E2E_SUBJECT,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * A node's links, grouped by what they mean (S7a, #342; ONTOLOGY §6.3,
 * §6.5, §14): "Used in" on a source, an Article's "Bibliography", what an
 * Area contains, "Mentioned in"; the warning when a node that nodes in use
 * still draw on is deleted or archived; and the Unlinked Mentions tool.
 *
 * Every title starts with PREFIX. Nodes are deleted by id (an archived one
 * isn't in the lookup the prefix wipe uses), and the prefix wipe runs
 * before AND after each test for whatever a crashed attempt left behind.
 */

const PREFIX = 'e2e conn ';

const wipe = (request: APIRequestContext) => deleteNodesByPrefix(request, PREFIX);

/** Counts the reads of a node's links the page makes from here on. */
async function countLinkReads(page: Page, id: string) {
  const reads = { count: 0 };
  await page.route(`**/api/nodes/${id}/edges`, (route) => {
    if (route.request().method() === 'GET') reads.count += 1;
    return route.fallback();
  });
  return reads;
}

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});

type Made = { id: string; title: string };

async function link(request: APIRequestContext, source: Made, target: Made, edge_type: string) {
  const made = await request.post('/api/edges', {
    data: { source_id: source.id, target_id: target.id, edge_type },
  });
  expect(made.status(), `${edge_type}: ${await made.text()}`).toBe(201);
  return (await made.json()).id as string;
}

async function archive(request: APIRequestContext, node: Made) {
  const saved = await request.put(`/api/nodes/${node.id}`, { data: { status: 'archived' } });
  expect(saved.ok()).toBeTruthy();
}

/** Opens a node's page and its Connections section. */
async function openConnections(page: Page, id: string) {
  await page.goto(`/nodes/${id}`);
  const section = page.getByTestId('node-connections');
  await section.getByRole('button', { name: /Connections/ }).click();
  return section;
}

const group = (page: Page, slug: string) => page.getByTestId(`connections-${slug}`);
const rows = (page: Page, slug: string) => group(page, slug).getByTestId('connection-row');

test('links are grouped by what they mean, with nodes out of use last', async ({ page, request }) => {
  const stamp = Date.now();
  const make = (name: string, node_type: string) =>
    createNode(request, `${PREFIX}${name} ${stamp}`, { node_type });
  const source = await make('source', 'reference');
  const essay = await make('essay', 'article');
  const project = await make('project', 'project');
  const retired = await make('a retired essay', 'article');
  const area = await make('area', 'area');
  const made = [source, essay, project, retired, area];
  try {
    await link(request, essay, source, 'references');
    await link(request, project, source, 'references');
    await link(request, retired, source, 'references');
    await link(request, area, project, 'contains');
    await link(request, essay, project, 'derived_from');
    await archive(request, retired);

    // The source: every node that draws on it, the archived one last
    // although its title sorts first, and marked.
    await openConnections(page, source.id);
    await expect(group(page, 'used-in').getByRole('heading')).toContainText('Used in');
    await expect(rows(page, 'used-in')).toHaveText([
      new RegExp(`${essay.title}`),
      new RegExp(`${project.title}`),
      new RegExp(`${retired.title}.*Archived`),
    ]);
    await expect(rows(page, 'used-in').nth(0).getByTestId('connection-standing')).toHaveCount(0);
    // Group headings are not page sections: the outline's `h2`s stay as
    // they were (node_page.spec.ts counts them).
    await expect(group(page, 'used-in').locator('h2')).toHaveCount(0);
    // A Reference has no home, and none of these links is one.
    await expect(group(page, 'home')).toHaveCount(0);

    // An Article's sources are its bibliography; what it came from is
    // its lineage.
    await openConnections(page, essay.id);
    await expect(group(page, 'sources').getByRole('heading')).toContainText('Bibliography');
    await expect(rows(page, 'sources')).toHaveText([new RegExp(source.title)]);
    await expect(group(page, 'derived-from').getByRole('heading')).toContainText('Derived from');
    await expect(rows(page, 'derived-from')).toHaveText([new RegExp(project.title)]);

    // A Project's are just its sources. Its home is the row above the
    // list, never a second "Home" group in it.
    await openConnections(page, project.id);
    await expect(group(page, 'sources').getByRole('heading')).toContainText('Sources');
    await expect(group(page, 'descendants').getByRole('heading')).toContainText('Descendants');
    await expect(rows(page, 'descendants')).toHaveText([new RegExp(essay.title)]);
    await expect(page.getByTestId('node-home').getByRole('link', { name: area.title })).toBeVisible();
    await expect(group(page, 'home')).toHaveCount(0);

    // An Area lists what it contains; a row is a link to the node.
    await openConnections(page, area.id);
    await expect(rows(page, 'contains')).toHaveText([new RegExp(project.title)]);
    await rows(page, 'contains').getByRole('link', { name: new RegExp(project.title) }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${project.id}$`));

    // Removing a link takes it off the page and out of the data.
    await openConnections(page, essay.id);
    await group(page, 'sources')
      .getByRole('button', { name: `Remove the link to ${source.title}` })
      .click();
    await expect(group(page, 'sources')).toHaveCount(0);
    const left: { edge_type: string }[] = await (
      await request.get(`/api/nodes/${essay.id}/edges`)
    ).json();
    expect(left.map((e) => e.edge_type)).toEqual(['derived_from']);
  } finally {
    for (const n of made) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('a long group folds, and stays open across a change', async ({ page, request }) => {
  const stamp = Date.now();
  const source = await createNode(request, `${PREFIX}busy source ${stamp}`, { node_type: 'reference' });
  const users: Made[] = [];
  try {
    for (let i = 0; i < 16; i++) {
      const n = await createNode(request, `${PREFIX}user ${String(i).padStart(2, '0')} ${stamp}`);
      users.push(n);
      await link(request, n, source, 'references');
    }
    await openConnections(page, source.id);
    await expect(rows(page, 'used-in')).toHaveCount(12);
    await group(page, 'used-in').getByRole('button', { name: 'Show 4 more…' }).click();
    await expect(rows(page, 'used-in')).toHaveCount(16);
    // Removing one reloads the list; the group stays open.
    await group(page, 'used-in')
      .getByRole('button', { name: `Remove the link to ${users[15].title}` })
      .click();
    await expect(rows(page, 'used-in')).toHaveCount(15);
    await expect(group(page, 'used-in').getByRole('button', { name: 'Show fewer' })).toBeVisible();
  } finally {
    for (const n of [source, ...users]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('a remove control is offered only to someone who may remove the link', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const theirs = await createNode(otherRequest, `${PREFIX}their essay ${stamp}`);
  const cited = await createNode(otherRequest, `${PREFIX}their source ${stamp}`, {
    node_type: 'reference',
  });
  const hidden = await createNode(otherRequest, `${PREFIX}their private source ${stamp}`, {
    node_type: 'reference',
  });
  try {
    await link(otherRequest, theirs, cited, 'references');
    await link(otherRequest, theirs, hidden, 'references');
    await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'viewer');
    await shareNode(otherRequest, cited.id, E2E_SUBJECT, 'viewer');

    // A viewer reads the link whose two ends they can see, and only it.
    const ownGrants = () =>
      page.waitForResponse((r) => new URL(r.url()).pathname === `/api/nodes/${theirs.id}/permissions`);
    let asked = ownGrants();
    const section = await openConnections(page, theirs.id);
    await expect(rows(page, 'sources')).toHaveText([new RegExp(cited.title)]);
    await expect(section).not.toContainText(hidden.title);
    await expect(section.getByRole('button', { name: /Remove the link/ })).toHaveCount(0);
    // Nor are they offered the form to add one: the page has read their
    // grant (a viewer's), and the server would refuse.
    expect((await asked).ok()).toBeTruthy();
    const add = section.locator('button[title="Add Edge"]');
    await expect(add).toHaveCount(0);

    // An editor of the link's source may remove it, and is offered it.
    await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'editor');
    asked = ownGrants();
    await openConnections(page, theirs.id);
    await expect(
      group(page, 'sources').getByRole('button', { name: `Remove the link to ${cited.title}` }),
    ).toBeVisible();
    await expect(section).not.toContainText(hidden.title);
    // And the form to add one, which works for them.
    expect((await asked).ok()).toBeTruthy();
    await add.click();
    await expect(page.getByRole('combobox', { name: 'Search for a node…' })).toBeVisible();

    // If their grant can't be read, nothing is offered on a guess.
    await page.route(`**/api/nodes/${theirs.id}/permissions`, (route) =>
      route.fulfill({ status: 500, body: 'no' }),
    );
    asked = ownGrants();
    await openConnections(page, theirs.id);
    await expect(rows(page, 'sources')).toHaveText([new RegExp(cited.title)]);
    await asked;
    await expect(add).toHaveCount(0);
    await page.unroute(`**/api/nodes/${theirs.id}/permissions`);
  } finally {
    for (const n of [theirs, cited, hidden]) await otherRequest.delete(`/api/nodes/${n.id}`);
  }
});

test('a shared editor can take a node out of its home, from the list', async ({
  page,
  otherRequest,
}) => {
  const stamp = Date.now();
  const area = await createNode(otherRequest, `${PREFIX}their area ${stamp}`, { node_type: 'area' });
  const project = await createNode(otherRequest, `${PREFIX}their project ${stamp}`, {
    node_type: 'project',
  });
  try {
    await link(otherRequest, area, project, 'contains');
    await shareNode(otherRequest, area.id, E2E_SUBJECT, 'viewer');
    await shareNode(otherRequest, project.id, E2E_SUBJECT, 'editor');

    // The home row's buttons are the owner's. A home never traps a
    // child, so its editor is offered the link's removal in the list.
    await openConnections(page, project.id);
    const home = page.getByTestId('node-home');
    await expect(home.getByRole('link', { name: area.title })).toBeVisible();
    await expect(home.getByRole('button')).toHaveCount(0);
    await group(page, 'home')
      .getByRole('button', { name: `Remove the link to ${area.title}` })
      .click();
    await expect(home).toContainText('None: it stands alone');
    await expect(group(page, 'home')).toHaveCount(0);
    const left: unknown[] = await (await otherRequest.get(`/api/nodes/${project.id}/edges`)).json();
    expect(left).toEqual([]);
  } finally {
    for (const n of [area, project]) await otherRequest.delete(`/api/nodes/${n.id}`);
  }
});

test('a failed read of the links is an error with a retry, never "no connections"', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const a = await createNode(request, `${PREFIX}flaky a ${stamp}`);
  const b = await createNode(request, `${PREFIX}flaky b ${stamp}`, { node_type: 'reference' });
  try {
    await link(request, a, b, 'references');
    await page.route(`**/api/nodes/${a.id}/edges`, (route) => route.fulfill({ status: 500, body: 'no' }));
    const section = await openConnections(page, a.id);
    await expect(section.getByTestId('load-error')).toContainText('Couldn’t load this node’s connections');
    await expect(section.getByTestId('connections-empty')).toHaveCount(0);
    await page.unroute(`**/api/nodes/${a.id}/edges`);
    await section.getByRole('button', { name: 'Try again' }).click();
    await expect(rows(page, 'sources')).toHaveText([new RegExp(b.title)]);

    // The home row says so too when its own read fails: a blank row
    // would read as "no home".
    await page.route(`**/api/nodes/${a.id}/home`, (route) => route.fulfill({ status: 500, body: 'no' }));
    await openConnections(page, a.id);
    const home = page.getByTestId('node-home');
    await expect(home.getByRole('alert')).toHaveText('Couldn’t load the home.');
    await page.unroute(`**/api/nodes/${a.id}/home`);
    await home.getByRole('button', { name: 'Try again' }).click();
    await expect(home).toContainText('None: it stands alone');
  } finally {
    for (const n of [a, b]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('a node that names this one is found, linked, and then listed under "Mentioned in"', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  // One odd word, so the search finds only this spec's nodes.
  const word = `quillwort${stamp}`;
  const target = await createNode(request, `${PREFIX}${word}`);
  const linked = await createNode(request, `${PREFIX}already links ${stamp}`);
  const plain = await createNode(request, `${PREFIX}only names it ${stamp}`, {
    body: `A line about ${PREFIX}${word} in plain words.`,
  });
  try {
    // A save syncs the text's [[links]] into wiki links.
    const saved = await request.put(`/api/nodes/${linked.id}`, {
      data: { body: `See [[${target.title}]] for more.` },
    });
    expect(saved.ok()).toBeTruthy();

    const reads = await countLinkReads(page, target.id);
    const section = await openConnections(page, target.id);
    await expect(rows(page, 'mentioned-in')).toHaveText([new RegExp(linked.title)]);
    // A wiki link mirrors the text: it is never removed by hand.
    await expect(section.getByRole('button', { name: /Remove the link/ })).toHaveCount(0);

    // The outline has the section under its new name.
    const mentions = page.getByTestId('node-unlinked-mentions');
    await mentions.getByRole('button', { name: /Unlinked Mentions/ }).click();
    const found = mentions.getByTestId('unlinked-mention');
    await expect(found.filter({ hasText: plain.title })).toBeVisible();
    // The node that already links here is not an unlinked mention.
    await expect(found.filter({ hasText: linked.title })).toHaveCount(0);
    // Both sections are open on one read of the node's links.
    expect(reads.count).toBe(1);

    await mentions.getByRole('button', { name: `Link the mention in ${plain.title}` }).click();
    await expect(page.getByText('Mention linked.')).toBeVisible();
    // It leaves the list at once, not a round trip later with a live
    // button ...
    await expect(found.filter({ hasText: plain.title })).toHaveCount(0);
    // ... and one more read of the links puts it under "Mentioned in".
    await expect(rows(page, 'mentioned-in')).toHaveText([
      new RegExp(linked.title),
      new RegExp(plain.title),
    ]);
    expect(reads.count).toBe(2);
    // The list as it is once the search has run again: still without it.
    await expect(mentions.getByTestId('mentions-loading')).toHaveCount(0);
    await expect(found.filter({ hasText: plain.title })).toHaveCount(0);
    const body = (await (await request.get(`/api/nodes/${plain.id}`)).json()).body as string;
    expect(body).toContain(`[[${target.title}]]`);
  } finally {
    for (const n of [target, linked, plain]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('deleting a source that nodes in use still draw on warns, apart from what is deleted', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const source = await createNode(request, `${PREFIX}cited ${stamp}`, { node_type: 'reference' });
  const essay = await createNode(request, `${PREFIX}citing ${stamp}`);
  try {
    await link(request, essay, source, 'references');
    await page.goto(`/nodes/${source.id}`);
    await page.getByRole('button', { name: 'Delete', exact: true }).first().click();
    const dialog = page.getByRole('alertdialog', { name: 'Confirm delete' });
    const warning = dialog.getByTestId('delete-warning');
    await expect(warning).toHaveText(
      /Still used as a source: 1 link from a node in use will go too\./,
    );
    // A warning, not a loss and not a block.
    await expect(dialog.getByTestId('delete-details')).not.toContainText('This also deletes:');
    await expect(dialog.getByRole('button', { name: 'Delete' })).toBeEnabled();
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();

    // Once the citing node is out of use, nothing is said.
    await archive(request, essay);
    await page.getByRole('button', { name: 'Delete', exact: true }).first().click();
    await expect(dialog.getByRole('button', { name: 'Delete' })).toBeEnabled();
    await expect(warning).toHaveCount(0);
    await dialog.getByRole('button', { name: 'Cancel' }).click();
  } finally {
    for (const n of [source, essay]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('archiving a source says how many nodes in use still draw on it, or that it can’t tell', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const source = await createNode(request, `${PREFIX}to archive ${stamp}`, { node_type: 'reference' });
  const essay = await createNode(request, `${PREFIX}still citing ${stamp}`);
  try {
    await link(request, essay, source, 'references');
    const reads = await countLinkReads(page, source.id);
    await page.goto(`/nodes/${source.id}/edit`);
    const state = page.getByRole('radiogroup', { name: 'Lifecycle state' });
    const notice = page.getByTestId('archive-notice');
    await expect(state).toBeVisible();
    // Nothing is read, and nothing said, until Archived is chosen.
    await page.waitForLoadState('networkidle');
    await expect(notice).toBeEmpty();
    expect(reads.count).toBe(0);

    // A read that fails never reads as "nothing uses it".
    await page.route(`**/api/nodes/${source.id}/edges`, (route) =>
      route.fulfill({ status: 500, body: 'no' }),
    );
    await state.getByRole('radio', { name: 'Archived' }).click();
    await expect(notice).toHaveText(
      'Couldn’t check whether nodes in use still draw on this as a source.',
    );
    await page.unroute(`**/api/nodes/${source.id}/edges`);

    await state.getByRole('radio', { name: 'Draft' }).click();
    await expect(notice).toBeEmpty();
    await state.getByRole('radio', { name: 'Archived' }).click();
    await expect(notice).toHaveText('1 node in use still draws on this as a source. The link stays.');
    // A notice, not a block: the editor saves the new state on its own.
    await expect
      .poll(async () => (await (await request.get(`/api/nodes/${source.id}`)).json()).status, {
        timeout: 15_000,
      })
      .toBe('archived');
    const kept: unknown[] = await (await request.get(`/api/nodes/${source.id}/edges`)).json();
    expect(kept).toHaveLength(1);
  } finally {
    for (const n of [source, essay]) await request.delete(`/api/nodes/${n.id}`);
  }
});
