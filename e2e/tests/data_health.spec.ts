import { test, expect, createNode, type APIRequestContext } from './fixtures';

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

async function wipe(request: APIRequestContext) {
  // Title lookup (own nodes, prefix first); a few rounds in case more
  // leftovers exist than one page returns.
  for (let round = 0; round < 5; round++) {
    const hits: { id: string; title: string }[] = await (
      await request.get(`/api/nodes/lookup?q=${encodeURIComponent(PREFIX.trim())}&limit=20`)
    ).json();
    const mine = hits.filter((n) => n.title.startsWith(PREFIX));
    if (mine.length === 0) break;
    for (const n of mine) await request.delete(`/api/nodes/${n.id}`);
  }
  const inbox = await (await request.get('/api/tasks/inbox')).json();
  for (const t of inbox ?? []) {
    if (typeof t.title === 'string' && t.title.startsWith(PREFIX)) {
      await request.delete(`/api/tasks/${t.id}`);
    }
  }
}

test('the page lists data that breaks the model, linked to where it lives', async ({
  page,
  request,
  otherRequest,
}) => {
  await wipe(request);
  try {
    const stamp = Date.now();
    const area = await createNode(request, `${PREFIX}area ${stamp}`, { node_type: 'area' });
    const a = await createNode(request, `${PREFIX}pair a ${stamp}`, { node_type: 'project' });
    const b = await createNode(request, `${PREFIX}pair b ${stamp}`, { node_type: 'project' });
    // A task on an Area is refused now (§3.2, #216): it can no longer be
    // made through the API, so that rule is covered by the pg tests, and
    // here the refusal itself is checked.
    const onArea = await request.post(`/api/nodes/${area.id}/tasks`, {
      data: { title: `${PREFIX}task on area ${stamp}` },
    });
    expect(onArea.status()).toBe(422);
    expect((await onArea.json()).error).toBe('only Projects have tasks');
    // A task with no Project at all.
    const orphanResp = await request.post('/api/tasks', {
      data: { title: `${PREFIX}orphan ${stamp}` },
    });
    expect(orphanResp.ok()).toBeTruthy();
    const orphan = await orphanResp.json();
    // related_to stored both ways.
    for (const [s, t] of [
      [a.id, b.id],
      [b.id, a.id],
    ]) {
      const edge = await request.post('/api/edges', {
        data: { source_id: s, target_id: t, edge_type: 'related_to' },
      });
      expect(edge.ok()).toBeTruthy();
    }

    // The API reports each under its rule, to the owner only.
    expect(await subjectsOf(request, 'orphan_task')).toContain(orphan.id);
    const pair = await subjectsOf(request, 'related_to_both_ways');
    expect(pair.includes(a.id) || pair.includes(b.id)).toBeTruthy();
    for (const rule of ['orphan_task', 'related_to_both_ways']) {
      const theirs = await subjectsOf(otherRequest, rule);
      for (const id of [orphan.id, a.id, b.id]) expect(theirs).not.toContain(id);
    }

    // The page shows them, each in its rule's card, linking to the item.
    await page.goto('/data-health');
    await expect(page.getByRole('heading', { name: 'Data health' })).toBeVisible();
    const orphanCard = page.getByTestId('data-health-orphan_task');
    await expect(orphanCard.locator(`a[href="/tasks/inbox?task=${orphan.id}"]`)).toBeVisible();
    // The pair this test made, by its own nodes.
    const pairCard = page.getByTestId('data-health-related_to_both_ways');
    await expect(pairCard.locator(`a[href="/nodes/${a.id}"]`)).toBeVisible();
    await expect(pairCard.locator(`a[href="/nodes/${b.id}"]`)).toBeVisible();
    // Each card explains its rule and how to fix it.
    await expect(orphanCard).toContainText('A task belongs to exactly one Project.');
    await expect(orphanCard).toContainText('To fix:');

    // A link opens the item where it lives.
    await pairCard.locator(`a[href="/nodes/${a.id}"]`).first().click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${a.id}`));
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
    await expect(card).toContainText('Only Projects have tasks.');
    await expect(card.locator(`a[href="/nodes/${node.id}?task=${taskId}"]`)).toBeVisible();
    await expect(card.locator(`a[href="/nodes/${node.id}"]`)).toBeVisible();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('"Move to a Project" fixes an orphan task in one step (#216)', async ({ page, request }) => {
  await wipe(request);
  try {
    const stamp = Date.now();
    const project = await createNode(request, `${PREFIX}move target ${stamp}`, {
      node_type: 'project',
    });
    const article = await createNode(request, `${PREFIX}move article ${stamp}`);
    const orphan = await (
      await request.post('/api/tasks', { data: { title: `${PREFIX}to move ${stamp}` } })
    ).json();

    // The API itself refuses the wrong destination.
    const refused = await request.patch(`/api/tasks/${orphan.id}`, {
      data: { node_id: article.id },
    });
    expect(refused.status()).toBe(422);

    await page.goto('/data-health');
    const card = page.getByTestId('data-health-orphan_task');
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
    await expect(card.getByText(`${PREFIX}to move ${stamp}`)).toHaveCount(0);
    await expect
      .poll(async () => (await (await request.get(`/api/nodes/${project.id}/tasks`)).json()).length)
      .toBe(1);
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
