import { test, expect, createNode, type APIRequestContext, type Page } from './fixtures';

/**
 * A project's status as a structured value (#143): the header chip and its
 * picker, one spelling per status, the template migration on create, and
 * the Projects list's badge and filter.
 */

type Node = { id: string; title: string; body: string | null; project_status: string | null };

const getNode = async (request: APIRequestContext, id: string): Promise<Node> =>
  (await request.get(`/api/nodes/${id}`)).json();

/** The header chip: its name is "Status: X. Change status" or "Set status". */
const chip = (page: Page) => page.getByTestId('project-status').getByRole('button');
const picker = (page: Page) => page.getByTestId('status-picker');

async function openPicker(page: Page) {
  await chip(page).click();
  await expect(picker(page).getByRole('combobox', { name: 'Status' })).toBeFocused();
}

test('a new project starts Active and the chip sets, reuses and clears its status', async ({
  page,
  request,
}) => {
  const node = await createNode(request, `e2e status chip ${Date.now()}`, { node_type: 'project' });
  try {
    expect((await getNode(request, node.id)).project_status).toBe('Active');
    await page.goto(`/nodes/${node.id}`);
    await expect(chip(page)).toHaveAccessibleName('Status: Active. Change status');

    // A new value: typed, offered as "Use …", committed with Enter.
    await openPicker(page);
    await page.keyboard.type('On ice');
    await expect(picker(page).getByRole('option', { selected: true })).toHaveText(/Use .On ice./);
    await page.keyboard.press('Enter');
    await expect(chip(page)).toHaveAccessibleName('Status: On ice. Change status');
    await expect.poll(async () => (await getNode(request, node.id)).project_status).toBe('On ice');

    // Another case of an existing value offers only the existing spelling.
    await openPicker(page);
    await page.keyboard.type('ON ICE');
    const options = picker(page).getByRole('option');
    await expect(options).toHaveCount(1);
    // The accessible name skips the aria-hidden glyph (its text is in the DOM).
    await expect(options).toHaveAccessibleName('On ice');

    // Escape closes without saving, and stays on the page.
    await page.keyboard.press('Escape');
    await expect(picker(page)).toHaveCount(0);
    expect(new URL(page.url()).pathname).toBe(`/nodes/${node.id}`);

    // Clear.
    await openPicker(page);
    await picker(page).getByRole('button', { name: 'Clear status' }).click();
    await expect(chip(page)).toHaveAccessibleName('Set status');
    await expect.poll(async () => (await getNode(request, node.id)).project_status).toBeNull();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

type Activity = { action: string; metadata: Record<string, unknown> };

test('a status change is recorded once in Activity, as from → to (#208)', async ({
  page,
  request,
}) => {
  const node = await createNode(request, `e2e status history ${Date.now()}`, {
    node_type: 'project',
  });
  try {
    await page.goto(`/nodes/${node.id}`);
    await expect(chip(page)).toHaveAccessibleName('Status: Active. Change status');
    await openPicker(page);
    await page.keyboard.type('Blocked');
    await page.keyboard.press('Enter');
    await expect(chip(page)).toHaveAccessibleName('Status: Blocked. Change status');

    // The API: exactly one status_changed, and the chip's save is not also
    // an "edited" (the status-only save is History's line on its own).
    await expect
      .poll(async () => {
        const log: Activity[] = await (await request.get(`/api/nodes/${node.id}/activity`)).json();
        return log.map((e) => e.action).sort();
      })
      .toEqual(['created', 'status_changed']);
    const log: Activity[] = await (await request.get(`/api/nodes/${node.id}/activity`)).json();
    const change = log.find((e) => e.action === 'status_changed');
    expect(change?.metadata).toMatchObject({ from: 'Active', to: 'Blocked' });
    expect(log.find((e) => e.action === 'created')?.metadata).toMatchObject({
      project_status: 'Active',
    });

    // The panel shows it as one line.
    const section = page.getByTestId('node-activity');
    const toggle = section.locator('button[aria-expanded]').first();
    if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
    await expect(section.getByText('Active \u2192 Blocked')).toHaveCount(1);
    await expect(section.getByText(/changed status/)).toHaveCount(1);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test("a template's Status section becomes the field, leaving its narrative", async ({ request }) => {
  const node = await createNode(request, `e2e status template ${Date.now()}`, {
    node_type: 'project',
    body: '## Status\n\n**BLOCKED**\n\nWaiting on legal.\n\n## Goals\n',
  });
  try {
    const n = await getNode(request, node.id);
    expect(n.project_status).toBe('BLOCKED');
    expect(n.body).toBe('## Status\n\nWaiting on legal.\n\n## Goals\n');
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('the Projects list shows the status and filters by it', async ({ page, request }) => {
  const stamp = Date.now();
  const status = `e2e filter ${stamp}`.slice(0, 40);
  const tagged = await createNode(request, `e2e status tagged ${stamp}`, { node_type: 'project' });
  const other = await createNode(request, `e2e status other ${stamp}`, { node_type: 'project' });
  try {
    const put = await request.put(`/api/nodes/${tagged.id}`, { data: { project_status: status } });
    expect(put.ok()).toBeTruthy();

    await page.goto('/nodes');
    await expect(page.locator('main')).toBeVisible();
    await page.locator('button[title="Projects"]').first().click();
    const row = (title: string) => page.locator('li', { hasText: title });
    await expect(row(tagged.title).getByTestId('project-status-badge')).toHaveText(status);
    await expect(row(other.title).getByTestId('project-status-badge')).toHaveText('Active');

    const filter = page.getByTestId('project-status-filter');
    await filter.getByRole('button', { name: /Project status: Any/ }).click();
    await page.getByRole('menu', { name: 'Project status' }).getByRole('menuitem', { name: status }).click();
    await expect(row(tagged.title)).toBeVisible();
    await expect(row(other.title)).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${tagged.id}`);
    await request.delete(`/api/nodes/${other.id}`);
  }
});
