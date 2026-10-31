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
    // A task on an Area (only Projects have tasks).
    const onArea = await request.post(`/api/nodes/${area.id}/tasks`, {
      data: { title: `${PREFIX}task on area ${stamp}` },
    });
    expect(onArea.ok()).toBeTruthy();
    const areaTask = await onArea.json();
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
    expect(await subjectsOf(request, 'task_on_non_project')).toContain(areaTask.id);
    expect(await subjectsOf(request, 'orphan_task')).toContain(orphan.id);
    const pair = await subjectsOf(request, 'related_to_both_ways');
    expect(pair.includes(a.id) || pair.includes(b.id)).toBeTruthy();
    for (const rule of ['task_on_non_project', 'orphan_task', 'related_to_both_ways']) {
      const theirs = await subjectsOf(otherRequest, rule);
      for (const id of [areaTask.id, orphan.id, a.id, b.id]) expect(theirs).not.toContain(id);
    }

    // The page shows them, each in its rule's card, linking to the item.
    await page.goto('/data-health');
    await expect(page.getByRole('heading', { name: 'Data health' })).toBeVisible();
    const onAreaCard = page.getByTestId('data-health-task_on_non_project');
    const areaTaskLink = onAreaCard.locator(`a[href="/nodes/${area.id}?task=${areaTask.id}"]`);
    await expect(areaTaskLink).toBeVisible();
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
    await areaTaskLink.click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${area.id}`));
  } finally {
    await wipe(request);
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
