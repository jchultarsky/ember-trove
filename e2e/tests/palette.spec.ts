import { test, expect, Page } from '@playwright/test';

/**
 * Cmd-K command palette: node search, app commands with synonym matching,
 * navigation dispatch, and node-context commands.
 */

const PALETTE_INPUT = 'Search nodes, or type a new title…'; // Unicode ellipsis!

/**
 * Navigate and wait for the WASM app to render before sending keyboard
 * shortcuts. On cold CI runners the bundle takes seconds to initialize —
 * a Cmd+K fired before the window listener registers is silently lost
 * (caught in the first CI run; local warm stacks always won the race).
 */
async function gotoApp(page: Page, path: string) {
  await page.goto(path);
  await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });
}

test('opens with Cmd+K, lists capture commands on empty query, closes on Esc', async ({ page }) => {
  await gotoApp(page, '/tasks/my-day');
  await page.keyboard.press('ControlOrMeta+k');

  const input = page.getByPlaceholder(PALETTE_INPUT);
  await expect(input).toBeVisible();
  await expect(page.getByRole('button', { name: /New task \(quick capture\)/ })).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(input).not.toBeVisible();
});

test('matches commands by synonym and dispatches navigation', async ({ page }) => {
  await gotoApp(page, '/tasks/my-day');
  await page.keyboard.press('ControlOrMeta+k');

  // "theme" is a keyword alias of "Toggle dark mode" — synonym matching.
  await page.getByPlaceholder(PALETTE_INPUT).fill('theme');
  await expect(page.getByRole('button', { name: /Toggle dark mode/ })).toBeVisible();

  // Navigation command actually navigates (Enter picks the highlighted row,
  // which is the first match).
  await page.getByPlaceholder(PALETTE_INPUT).fill('calendar');
  await expect(page.getByRole('button', { name: /Go to Calendar/ })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/tasks\/calendar$/);
  await expect(page).toHaveTitle('Calendar — Ember Trove');
});

test('Go to Search and Go to Webhooks commands navigate (palette parity)', async ({ page }) => {
  // The palette is the primary nav surface since `/` opens it — every routed
  // destination needs a Go-command. Search and Webhooks were the gaps
  // (2026-07-17 review / webhooks UI).
  await gotoApp(page, '/tasks/my-day');

  await page.keyboard.press('ControlOrMeta+k');
  await page.getByPlaceholder(PALETTE_INPUT).fill('advanced');
  await expect(page.getByRole('button', { name: /Go to Search/ })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/search$/);
  await expect(page).toHaveTitle('Search — Ember Trove');

  await page.keyboard.press('ControlOrMeta+k');
  await page.getByPlaceholder(PALETTE_INPUT).fill('hooks');
  await expect(page.getByRole('button', { name: /Go to Webhooks/ })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/webhooks$/);
  await expect(page).toHaveTitle('Webhooks — Ember Trove');
});

test('toggle dark mode flips the html class (and back)', async ({ page }) => {
  await gotoApp(page, '/tasks/my-day');
  const html = page.locator('html');
  await expect(html).not.toHaveClass(/dark/);

  for (const expectDark of [true, false]) {
    await page.keyboard.press('ControlOrMeta+k');
    await page.getByPlaceholder(PALETTE_INPUT).fill('dark');
    await page.getByRole('button', { name: /Toggle dark mode/ }).click();
    if (expectDark) {
      await expect(html).toHaveClass(/dark/);
    } else {
      await expect(html).not.toHaveClass(/dark/);
    }
  }
});

test('finds and opens a node by title search', async ({ page, request }) => {
  const title = `e2e palette-node ${Date.now()}`;
  const created = await request.post('/api/nodes', {
    data: { title, node_type: 'article', body: 'palette test', metadata: {}, status: 'draft' },
  });
  expect(created.ok()).toBeTruthy();
  const node = await created.json();

  try {
    await gotoApp(page, '/tasks/my-day');
    await page.keyboard.press('ControlOrMeta+k');
    await page.getByPlaceholder(PALETTE_INPUT).fill(title);

    const row = page.getByRole('button', { name: new RegExp(`${node.title}.*Open`) });
    await expect(row).toBeVisible();
    await row.click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('offers node-context commands on a node page', async ({ page, request }) => {
  const title = `e2e palette-ctx ${Date.now()}`;
  const created = await request.post('/api/nodes', {
    data: { title, node_type: 'article', body: '', metadata: {}, status: 'draft' },
  });
  const node = await created.json();

  try {
    await gotoApp(page, `/nodes/${node.id}`);
    await page.keyboard.press('ControlOrMeta+k');
    // Empty query on a node page surfaces the context commands.
    await expect(page.getByRole('button', { name: /Edit current node/ })).toBeVisible();
    await expect(page.getByRole('button', { name: /Duplicate current node/ })).toBeVisible();

    // "Edit current node" navigates to the editor.
    await page.getByRole('button', { name: /Edit current node/ }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}/edit$`));
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('command-intent queries beat body-text node matches', async ({ page, request }) => {
  // A node whose BODY mentions "dark" but whose title does not — before the
  // v2.21.4 ranking fix this outranked the command and hijacked Enter.
  const title = `e2e bait ${Date.now()}`;
  const created = await request.post('/api/nodes', {
    data: {
      title,
      node_type: 'article',
      body: 'all about dark mode preferences and dark themes',
      metadata: {},
      status: 'draft',
    },
  });
  expect(created.ok()).toBeTruthy();
  const node = await created.json();

  try {
    await gotoApp(page, '/tasks/my-day');
    const html = page.locator('html');
    await expect(html).not.toHaveClass(/dark/);

    await page.keyboard.press('ControlOrMeta+k');
    await page.getByPlaceholder(PALETTE_INPUT).fill('dark');
    // The bait node shows up in the list (the scenario is real)…
    await expect(page.getByRole('button', { name: new RegExp(title) })).toBeVisible();
    // …but Enter runs the highlighted FIRST row, which must be the command.
    await page.keyboard.press('Enter');
    await expect(html).toHaveClass(/dark/);
    await expect(page).toHaveURL(/\/tasks\/my-day$/);

    // Restore the theme.
    await page.keyboard.press('ControlOrMeta+k');
    await page.getByPlaceholder(PALETTE_INPUT).fill('dark');
    await page.getByRole('button', { name: /Toggle dark mode/ }).click();
    await expect(html).not.toHaveClass(/dark/);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
