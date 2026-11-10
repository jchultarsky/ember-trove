import { test, expect, createNode } from './fixtures';

/**
 * The node list reaches every node (#257). It showed the first 50 of every
 * kind and filtered them by kind in the browser, so a kind's page could
 * miss nodes and nothing offered the rest. The server filters and sorts
 * now, and the list pages with "Load more".
 */

test('a kind page lists that kind from the server and loads more', async ({ page, request }) => {
  const stamp = Date.now();
  const made: { id: string }[] = [];
  try {
    // 52 Areas: two more than one page.
    for (let i = 0; i < 52; i++) {
      made.push(
        await createNode(request, `e2e paging ${stamp} ${String(i).padStart(2, '0')}`, {
          node_type: 'area',
        }),
      );
    }

    // The API applies the kind before paging, and sorts the whole list.
    const first = await (
      await request.get('/api/nodes?node_type=area&sort=title_asc&per_page=200')
    ).json();
    expect(first.nodes.every((n: { node_type: string }) => n.node_type === 'area')).toBe(true);
    const ours = first.nodes
      .map((n: { title: string }) => n.title)
      .filter((t: string) => t.startsWith(`e2e paging ${stamp}`));
    expect(ours).toHaveLength(52);
    expect(ours).toEqual([...ours].sort());
    expect(first.total).toBeGreaterThanOrEqual(52);

    await page.goto('/nodes');
    const asked = page.waitForRequest(
      (r) => new URL(r.url()).pathname === '/api/nodes' && r.url().includes('node_type=area'),
    );
    await page.locator('button[title="Areas"]').click();
    await asked;

    // One page is shown, and the footer says how many there are.
    const more = page.getByTestId('node-list-more');
    await expect(more).toContainText(/Showing 50 of \d+/);
    const rows = page.getByRole('button', { name: new RegExp(`^e2e paging ${stamp} \\d\\d$`) });
    // Ours are the newest Areas, so the first page is 50 of them.
    await expect(rows).toHaveCount(50);

    await more.getByRole('button', { name: 'Load more' }).click();
    await expect(rows).toHaveCount(52);
  } finally {
    for (const n of made) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('an unknown sort is refused', async ({ request }) => {
  const resp = await request.get('/api/nodes?sort=by_magic');
  expect(resp.status()).toBeGreaterThanOrEqual(400);
  expect(resp.status()).toBeLessThan(500);
});
