import { test, expect, Page, APIRequestContext } from '@playwright/test';

/**
 * Graph view — the largest UI surface (ui/src/components/graph_view.rs) and,
 * until this spec, entirely uncovered by e2e. Interaction model under test:
 *   - nodes render as SVG <g data-node-id> groups
 *   - double-click on a node navigates to its page (single click is reserved
 *     for edge-create mode)
 *   - "Add Edge" toolbar button → click source → click target → New Edge
 *     dialog → Create persists via POST /api/edges
 *   - "Orphans only" lens hides every node that has any edge
 *
 * Suite is sequential and shares one DB: every test creates uniquely-titled
 * nodes and deletes them (and their edges) in `finally`.
 */

/** Collect WASM panics / JS exceptions for the whole test. */
function collectPageErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(String(err)));
  return errors;
}

async function createNode(request: APIRequestContext, title: string) {
  const resp = await request.post('/api/nodes', {
    data: { title, node_type: 'article', body: 'graph spec node', metadata: {}, status: 'draft' },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

/** The SVG group for one node on the canvas. */
const nodeG = (page: Page, id: string) => page.locator(`g[data-node-id="${id}"]`);

/**
 * Interact with a node via dispatchEvent rather than a positional click:
 * Playwright's actionability machinery (scroll-into-view + stability checks)
 * hangs on SVG canvas children, so real-mouse clicks time out. The events
 * still run the app's own on:click / on:dblclick handlers on the group.
 */
const dispatchOnNode = (page: Page, id: string, event: 'click' | 'dblclick') =>
  nodeG(page, id).dispatchEvent(event);

/** Wait until the graph canvas is interactive (toolbar rendered after load). */
async function gotoGraph(page: Page) {
  await page.goto('/graph');
  await expect(page.getByRole('button', { name: 'Fit' })).toBeVisible();
}

test.describe('graph view', () => {
  test('renders created nodes on the canvas', async ({ page, request }) => {
    const errors = collectPageErrors(page);
    const stamp = Date.now();
    const a = await createNode(request, `e2e graph a ${stamp}`);
    const b = await createNode(request, `e2e graph b ${stamp}`);
    try {
      await gotoGraph(page);
      await expect(nodeG(page, a.id)).toBeVisible();
      await expect(nodeG(page, b.id)).toBeVisible();
      expect(errors).toEqual([]);
    } finally {
      await request.delete(`/api/nodes/${a.id}`);
      await request.delete(`/api/nodes/${b.id}`);
    }
  });

  test('double-click on a node opens its page', async ({ page, request }) => {
    const errors = collectPageErrors(page);
    const title = `e2e graph nav ${Date.now()}`;
    const node = await createNode(request, title);
    try {
      await gotoGraph(page);
      await dispatchOnNode(page, node.id, 'dblclick');
      await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}`));
      await expect(page.getByText(title).first()).toBeVisible();
      expect(errors).toEqual([]);
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('creates an edge through Add Edge mode', async ({ page, request }) => {
    const errors = collectPageErrors(page);
    const stamp = Date.now();
    const src = await createNode(request, `e2e edge src ${stamp}`);
    const tgt = await createNode(request, `e2e edge tgt ${stamp}`);
    let edgeId: string | undefined;
    try {
      await gotoGraph(page);
      await page.getByRole('button', { name: 'Add Edge' }).click();

      // First click selects the source (amber dashed ring appears inside the
      // group), second click opens the New Edge dialog.
      await dispatchOnNode(page, src.id, 'click');
      await expect(nodeG(page, src.id).locator('circle')).toHaveCount(2);
      await dispatchOnNode(page, tgt.id, 'click');

      await expect(page.getByRole('heading', { name: 'New Edge' })).toBeVisible();
      await page.getByRole('button', { name: 'Create' }).click();
      await expect(page.getByRole('heading', { name: 'New Edge' })).not.toBeVisible();

      // Server state — not just the UI.
      const edges = await (await request.get('/api/edges')).json();
      const created = edges.find(
        (e: { source_id: string; target_id: string }) =>
          e.source_id === src.id && e.target_id === tgt.id,
      );
      expect(created).toBeTruthy();
      expect(created.edge_type).toBe('references');
      edgeId = created.id;
      expect(errors).toEqual([]);
    } finally {
      if (edgeId) await request.delete(`/api/edges/${edgeId}`);
      await request.delete(`/api/nodes/${src.id}`);
      await request.delete(`/api/nodes/${tgt.id}`);
    }
  });

  test('orphans-only lens hides linked nodes', async ({ page, request }) => {
    const errors = collectPageErrors(page);
    const stamp = Date.now();
    const a = await createNode(request, `e2e lens linked-a ${stamp}`);
    const b = await createNode(request, `e2e lens linked-b ${stamp}`);
    const orphan = await createNode(request, `e2e lens orphan ${stamp}`);
    let edgeId: string | undefined;
    try {
      const created = await request.post('/api/edges', {
        data: { source_id: a.id, target_id: b.id, edge_type: 'references' },
      });
      expect(created.ok()).toBeTruthy();
      edgeId = (await created.json()).id;

      await gotoGraph(page);
      await expect(nodeG(page, a.id)).toBeVisible();
      await expect(nodeG(page, orphan.id)).toBeVisible();

      await page.getByRole('button', { name: 'Orphans only' }).click();
      await expect(nodeG(page, orphan.id)).toBeVisible();
      await expect(nodeG(page, a.id)).not.toBeVisible();
      await expect(nodeG(page, b.id)).not.toBeVisible();

      // Toggle back (button text now carries the checkmark).
      await page.getByRole('button', { name: /Orphans only/ }).click();
      await expect(nodeG(page, a.id)).toBeVisible();
      expect(errors).toEqual([]);
    } finally {
      if (edgeId) await request.delete(`/api/edges/${edgeId}`);
      await request.delete(`/api/nodes/${a.id}`);
      await request.delete(`/api/nodes/${b.id}`);
      await request.delete(`/api/nodes/${orphan.id}`);
    }
  });
});
