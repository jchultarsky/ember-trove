import { test, expect, createNode } from './fixtures';

/**
 * Node page "Connections": the add-edge picker searches nodes by title
 * (shared `use_node_search` hook — debounced, latest query wins) and the
 * picked node becomes the edge target.
 */

test('the add-edge picker finds a node by title and links it', async ({ page, request }) => {
  const stamp = Date.now();
  const source = await createNode(request, `e2e links source ${stamp}`);
  const target = await createNode(request, `e2e links target ${stamp}`);
  try {
    await page.goto(`/nodes/${source.id}`);
    await page.getByRole('button', { name: /Connections/ }).click();
    await page.locator('button[title="Add Edge"]').click();

    const search = page.getByPlaceholder('Search for a node…');
    await search.fill(`links target ${stamp}`);
    const hit = page.getByRole('button', { name: `e2e links target ${stamp}`, exact: true });
    await hit.click();
    await expect(search).toHaveValue(`e2e links target ${stamp}`);

    await page.locator('button[title="Add edge"]').click();

    await expect
      .poll(async () => {
        const edges = await (await request.get(`/api/nodes/${source.id}/edges`)).json();
        return edges.map((e: { target_id: string }) => e.target_id);
      })
      .toContain(target.id);
  } finally {
    await request.delete(`/api/nodes/${source.id}`);
    await request.delete(`/api/nodes/${target.id}`);
  }
});
