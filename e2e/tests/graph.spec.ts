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
  // exact: graph nodes are role=button too, and a node title containing
  // "fit" would otherwise make this a strict-mode violation.
  await expect(page.getByRole('button', { name: 'Fit', exact: true })).toBeVisible();
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

  test('a node is keyboard-focusable, labelled, and Enter opens it', async ({ page, request }) => {
    // Keyboard/a11y baseline (keyboard phase 3): nodes are focusable buttons
    // with an accessible name, show a focus ring, and activate on Enter.
    const errors = collectPageErrors(page);
    const title = `e2e graph kbd ${Date.now()}`;
    const node = await createNode(request, title);
    try {
      await gotoGraph(page);
      const g = nodeG(page, node.id);
      await expect(g).toBeVisible();
      await expect(g).toHaveAttribute('role', 'button');
      await expect(g).toHaveAttribute('tabindex', '0');
      await expect(g).toHaveAttribute('aria-label', new RegExp(title));

      // Focusing the node adds the focus ring (article shape circle + ring = 2).
      await g.focus();
      await expect(g.locator('circle')).toHaveCount(2);

      // Enter activates → opens the node page.
      await page.keyboard.press('Enter');
      await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}`));
      expect(errors).toEqual([]);
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });

  test('auto-arrange clusters a star around its hub, not in rows', async ({ page, request }) => {
    // Regression for the BFS-row layout: arranging a hub with satellites must
    // produce a ring (spread in BOTH axes), keep satellites within spring
    // range of the hub, and persist the result via the batch positions API.
    const errors = collectPageErrors(page);
    const stamp = Date.now();
    const hub = await createNode(request, `e2e cluster hub ${stamp}`);
    const sats = [];
    for (let i = 0; i < 6; i++) sats.push(await createNode(request, `e2e cluster sat${i} ${stamp}`));
    const edgeIds: string[] = [];
    try {
      for (const s of sats) {
        const r = await request.post('/api/edges', {
          data: { source_id: hub.id, target_id: s.id, edge_type: 'references' },
        });
        expect(r.ok()).toBeTruthy();
        edgeIds.push((await r.json()).id);
      }

      await gotoGraph(page);
      await expect(nodeG(page, hub.id)).toBeVisible();
      await page.getByRole('button', { name: 'Auto-arrange' }).click();

      // Arrange saves every position in one batch; poll until ours land.
      const ids = [hub.id, ...sats.map((s) => s.id)];
      await expect
        .poll(
          async () => {
            const saved = await (await request.get('/api/graph/positions')).json();
            return ids.filter((id) => saved.some((p: { node_id: string }) => p.node_id === id))
              .length;
          },
          { timeout: 15_000 },
        )
        .toBe(ids.length);

      const saved = await (await request.get('/api/graph/positions')).json();
      const pos = new Map<string, { x: number; y: number }>(
        saved.map((p: { node_id: string; x: number; y: number }) => [p.node_id, p]),
      );
      const h = pos.get(hub.id)!;
      const satPos = sats.map((s) => pos.get(s.id)!);

      // Every satellite within spring range of the hub (ring band).
      for (const p of satPos) {
        const d = Math.hypot(p.x - h.x, p.y - h.y);
        expect(d).toBeGreaterThan(100);
        expect(d).toBeLessThan(700);
      }
      // A row collapses one axis; a ring spreads both.
      const stddev = (vals: number[]) => {
        const mean = vals.reduce((a, b) => a + b, 0) / vals.length;
        return Math.sqrt(vals.reduce((a, v) => a + (v - mean) ** 2, 0) / vals.length);
      };
      expect(stddev(satPos.map((p) => p.x))).toBeGreaterThan(40);
      expect(stddev(satPos.map((p) => p.y))).toBeGreaterThan(40);
      // Minimum readable spacing between all pairs.
      const all = [h, ...satPos];
      for (let a = 0; a < all.length; a++)
        for (let b = a + 1; b < all.length; b++)
          expect(Math.hypot(all[a].x - all[b].x, all[a].y - all[b].y)).toBeGreaterThan(60);
      expect(errors).toEqual([]);
    } finally {
      for (const id of edgeIds) await request.delete(`/api/edges/${id}`);
      await request.delete(`/api/nodes/${hub.id}`);
      for (const s of sats) await request.delete(`/api/nodes/${s.id}`);
    }
  });

  test('Fit brings far-away content into the viewport', async ({ page, request }) => {
    // Regression: Fit used to hard-reset to 100%/origin, which could leave a
    // clustered layout entirely off-screen. It must now frame the content.
    const errors = collectPageErrors(page);
    const stamp = Date.now();
    const a = await createNode(request, `e2e frame far-a ${stamp}`);
    const b = await createNode(request, `e2e frame far-b ${stamp}`);
    try {
      // Park both nodes far outside the default 100%/origin viewport.
      for (const [n, x, y] of [
        [a, 2600, 1700],
        [b, 3000, 1900],
      ] as const) {
        const r = await request.put(`/api/graph/positions/${n.id}`, { data: { x, y } });
        expect(r.ok()).toBeTruthy();
      }

      await gotoGraph(page);
      const viewport = page.viewportSize()!;
      // Sanity: at the default transform the nodes are off-screen.
      const before = await nodeG(page, a.id).boundingBox();
      expect(before && before.x < viewport.width && before.y < viewport.height).toBeFalsy();

      await page.getByRole('button', { name: 'Fit', exact: true }).click();
      // Assert on the node glyph (the article circle), not the whole <g>:
      // these machine-generated titles are far wider than any real-world
      // label and would poke past the fit padding by design.
      for (const n of [a, b]) {
        const box = await nodeG(page, n.id).locator('circle').first().boundingBox();
        expect(box).toBeTruthy();
        expect(box!.x).toBeGreaterThanOrEqual(0);
        expect(box!.y).toBeGreaterThanOrEqual(0);
        expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);
        expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
      }
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
