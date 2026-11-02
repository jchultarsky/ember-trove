import { test, expect, createNode } from './fixtures';

/**
 * #261: on desktop the editor header put the title input on the same row as
 * every control (type, status, save state, preview, Revert, Done…), which left
 * the title the narrowest thing on it. The title now has its own row and the
 * controls sit on the row below it.
 */
test.describe('editor header (#261)', () => {
  test('desktop: the title has its own, wide row above the controls', async ({ page, request }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    const node = await createNode(request, `e2e header ${Date.now()}`, { node_type: 'project' });
    try {
      await page.goto(`/nodes/${node.id}/edit`);
      // exact: the body's placeholder ("use [[Node Title]] to link nodes")
      // matches 'Title' as a substring too.
      const title = page.getByRole('textbox', { name: 'Title', exact: true });
      await expect(title).toHaveValue(node.title);
      // Something changed, so Revert joins the row too: the busiest header.
      await title.fill(`${node.title} renamed`);
      await expect(page.getByRole('button', { name: 'Revert' })).toBeVisible();

      const titleRow = page.getByTestId('editor-title-row');
      const controlsRow = page.getByTestId('editor-controls-row');
      const t = (await title.boundingBox())!;
      const tr = (await titleRow.boundingBox())!;
      const cr = (await controlsRow.boundingBox())!;

      // The controls row starts below the title row: they no longer share one.
      expect(cr.y).toBeGreaterThanOrEqual(tr.y + tr.height - 1);
      // The input takes most of its row (only the back arrow beside it).
      expect(t.width).toBeGreaterThan(tr.width * 0.8);
      await expect(page.getByRole('button', { name: 'Done', exact: true })).toBeVisible();
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });
});
