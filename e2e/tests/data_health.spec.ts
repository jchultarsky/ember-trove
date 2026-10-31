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

test('the page lists data that breaks the model, linked to where it lives', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const area = await createNode(request, `e2e dh area ${stamp}`, { node_type: 'area' });
  const a = await createNode(request, `e2e dh pair a ${stamp}`, { node_type: 'project' });
  const b = await createNode(request, `e2e dh pair b ${stamp}`, { node_type: 'project' });
  const taskIds: string[] = [];
  try {
    // A task on an Area (only Projects have tasks).
    const onArea = await request.post(`/api/nodes/${area.id}/tasks`, {
      data: { title: `e2e dh task on area ${stamp}` },
    });
    expect(onArea.ok()).toBeTruthy();
    const areaTask = await onArea.json();
    taskIds.push(areaTask.id);
    // A task with no Project at all.
    const orphanResp = await request.post('/api/tasks', {
      data: { title: `e2e dh orphan ${stamp}` },
    });
    expect(orphanResp.ok()).toBeTruthy();
    const orphan = await orphanResp.json();
    taskIds.push(orphan.id);
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
      expect(theirs).not.toContain(areaTask.id);
      expect(theirs).not.toContain(orphan.id);
      expect(theirs).not.toContain(a.id);
      expect(theirs).not.toContain(b.id);
    }

    // The page shows them, each in its rule's card, linking to the item.
    await page.goto('/data-health');
    await expect(page.getByRole('heading', { name: 'Data health' })).toBeVisible();
    const onAreaCard = page.getByTestId('data-health-task_on_non_project');
    await expect(
      onAreaCard.locator(`a[href="/nodes/${area.id}?task=${areaTask.id}"]`),
    ).toBeVisible();
    const orphanCard = page.getByTestId('data-health-orphan_task');
    const orphanLink = orphanCard.locator(`a[href="/tasks/inbox?task=${orphan.id}"]`);
    await expect(orphanLink).toBeVisible();
    await expect(
      page.getByTestId('data-health-related_to_both_ways').locator(`a[href^="/nodes/"]`).first(),
    ).toBeVisible();
    // Each card explains its rule and how to fix it.
    await expect(orphanCard).toContainText('A task belongs to exactly one Project.');
    await expect(orphanCard).toContainText('To fix:');

    // A link opens the item where it lives.
    await onAreaCard.locator(`a[href="/nodes/${area.id}?task=${areaTask.id}"]`).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${area.id}`));
  } finally {
    for (const id of taskIds) await request.delete(`/api/tasks/${id}`);
    for (const n of [area, a, b]) await request.delete(`/api/nodes/${n.id}`);
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
