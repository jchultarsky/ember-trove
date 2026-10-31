import {
  test,
  expect,
  createNode,
  shareNode,
  E2E_SUBJECT,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * Edge write rules (#224, ONTOLOGY §6, §14). New connections that break the
 * data model are refused with the reason; a home needs edit rights on both
 * nodes. Every node title starts with PREFIX; both identities' leftovers
 * are wiped before AND after each test (edges go with their nodes), so a
 * retry starts clean even after a crash.
 */

const PREFIX = 'e2e rules ';

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
  await wipe(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});

async function openAddEdge(page: Page, nodeId: string) {
  await page.goto(`/nodes/${nodeId}`);
  await page.getByRole('button', { name: /Connections/ }).click();
  await page.locator('button[title="Add Edge"]').click();
  return {
    combo: page.getByRole('combobox', { name: 'Search for a node…' }),
    chip: page.getByTestId('node-picker-chip'),
    type: page.locator('select:has(option[value="references"])'),
    confirm: page.locator('button[title="Add edge"]'),
  };
}

test('the Connections form says why a Reference cannot be contained', async ({ page, request }) => {
  const stamp = Date.now();
  const area = await createNode(request, `e2e rules area ${stamp}`, { node_type: 'area' });
  const reference = await createNode(request, `e2e rules ref ${stamp}`, { node_type: 'reference' });
  const project = await createNode(request, `e2e rules project ${stamp}`, { node_type: 'project' });
  try {
    const form = await openAddEdge(page, area.id);
    await form.combo.fill(`e2e rules ref ${stamp}`);
    await form.combo.press('Enter');
    await expect(form.chip).toContainText(`e2e rules ref ${stamp}`);
    await form.type.selectOption('contains');
    await form.confirm.click();
    await expect(
      page.getByText('A Reference is never contained; link it with “references” instead'),
    ).toBeVisible();

    // A Project is a fine child of an Area.
    await form.chip.getByRole('button', { name: 'Unlink node' }).click();
    await form.combo.fill(`e2e rules project ${stamp}`);
    await form.combo.press('Enter');
    await expect(form.chip).toContainText(`e2e rules project ${stamp}`);
    await form.type.selectOption('contains');
    await form.confirm.click();
    await expect
      .poll(async () => {
        const edges = await (await request.get(`/api/nodes/${area.id}/edges`)).json();
        return edges.map((e: { target_id: string; edge_type: string }) => `${e.edge_type}:${e.target_id}`);
      })
      .toContain(`contains:${project.id}`);
  } finally {
    for (const n of [area, reference, project]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('the API refuses loops, a second home and hand-made wiki links', async ({ request }) => {
  const stamp = Date.now();
  const p = await createNode(request, `e2e rules p ${stamp}`, { node_type: 'project' });
  const q = await createNode(request, `e2e rules q ${stamp}`, { node_type: 'project' });
  const a1 = await createNode(request, `e2e rules home1 ${stamp}`, { node_type: 'area' });
  const a2 = await createNode(request, `e2e rules home2 ${stamp}`, { node_type: 'area' });
  const post = (source_id: string, target_id: string, edge_type: string) =>
    request.post('/api/edges', { data: { source_id, target_id, edge_type } });
  try {
    expect((await post(p.id, q.id, 'depends_on')).status()).toBe(201);
    const loop = await post(q.id, p.id, 'depends_on');
    expect(loop.status()).toBe(422);
    expect((await loop.json()).error).toBe('this would make a loop of “depends on” links');

    expect((await post(a1.id, p.id, 'contains')).status()).toBe(201);
    const second = await post(a2.id, p.id, 'contains');
    expect(second.status()).toBe(422);
    expect((await second.json()).error).toBe('this node already has a home; remove that link first');

    const wiki = await post(p.id, q.id, 'wiki_link');
    expect(wiki.status()).toBe(422);
  } finally {
    for (const n of [p, q, a1, a2]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('a node shared for viewing can be linked but not given a home', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const mine = await createNode(request, `e2e rules my area ${stamp}`, { node_type: 'area' });
  const theirs = await createNode(otherRequest, `e2e rules their project ${stamp}`, {
    node_type: 'project',
  });
  try {
    await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'viewer');
    const post = (edge_type: string) =>
      request.post('/api/edges', {
        data: { source_id: mine.id, target_id: theirs.id, edge_type },
      });
    // §14: a home claims the child, so viewing it isn't enough.
    expect((await post('contains')).status()).toBe(403);
    // A plain link needs only view rights on the target.
    expect((await post('references')).status()).toBe(201);
  } finally {
    await request.delete(`/api/nodes/${mine.id}`);
    await otherRequest.delete(`/api/nodes/${theirs.id}`);
  }
});

// ── related_to is stored once (#226, ONTOLOGY §6.2) ──────────────────────────

test('a related link is one link: the reverse is flagged and both ends show ↔', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const a = await createNode(request, `e2e rules rel a ${stamp}`);
  const b = await createNode(request, `e2e rules rel b ${stamp}`);
  try {
    const made = await request.post('/api/edges', {
      data: { source_id: a.id, target_id: b.id, edge_type: 'related_to' },
    });
    expect(made.status()).toBe(201);

    // From b, linking back to a is the same link.
    const form = await openAddEdge(page, b.id);
    await form.combo.fill(`e2e rules rel a ${stamp}`);
    await form.combo.press('Enter');
    await expect(form.chip).toContainText(`e2e rules rel a ${stamp}`);
    await form.type.selectOption('related_to');
    await expect(page.getByTestId('edge-already-linked')).toHaveText('Already linked as related to');
    await expect(form.confirm).toBeDisabled();
    // The list shows it as ↔ with the other end, from b ...
    const row = (title: string) =>
      page.getByRole('button', { name: new RegExp(`↔.*related to.*${title}`) });
    await expect(row(`e2e rules rel a ${stamp}`)).toBeVisible();
    // ... and from a.
    await page.goto(`/nodes/${a.id}`);
    await page.getByRole('button', { name: /Connections/ }).click();
    await expect(row(`e2e rules rel b ${stamp}`)).toBeVisible();
  } finally {
    for (const n of [a, b]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('either end of a related link may delete it', async ({ request, otherRequest }) => {
  const stamp = Date.now();
  const mine = await createNode(request, `e2e rules rel mine ${stamp}`);
  const theirs = await createNode(otherRequest, `e2e rules rel theirs ${stamp}`);
  try {
    await shareNode(otherRequest, theirs.id, E2E_SUBJECT, 'viewer');
    const link = async (edge_type: string) => {
      const r = await request.post('/api/edges', {
        data: { source_id: mine.id, target_id: theirs.id, edge_type },
      });
      expect(r.status()).toBe(201);
      return (await r.json()).id as string;
    };
    const related = await link('related_to');
    const reference = await link('references');
    // The other identity edits only the target: a related link has no
    // direction, so it may remove it (§14) ...
    expect((await otherRequest.delete(`/api/edges/${related}`)).status()).toBe(204);
    // ... but a directed link belongs to its source's editors.
    expect((await otherRequest.delete(`/api/edges/${reference}`)).status()).toBe(403);
  } finally {
    await request.delete(`/api/nodes/${mine.id}`);
    await otherRequest.delete(`/api/nodes/${theirs.id}`);
  }
});
