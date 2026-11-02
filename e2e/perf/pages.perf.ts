import fs from 'node:fs';
import path from 'node:path';
import { test, expect, type Page } from '../tests/fixtures';

/**
 * Page loads and common actions, timed (performance phase P0, 2026-11-02).
 * Run against the local stack after `scripts/perf/seed.sh <scale>`; see
 * playwright.perf.config.ts. It records, per page and project:
 *
 * - mounted / auth: the app's own `performance.mark`s (ui/src/perf.rs);
 * - ready: the page's content is on screen (a page-specific locator);
 * - data: the last API response of the load has arrived;
 * - api / 429 / kB: API requests during the load, rate-limited ones, and
 *   API response bytes as sent (compressed if the server compressed).
 *
 * Each page is loaded twice in one context: `cold` (first visit) and `warm`
 * (the service worker and HTTP cache in play).
 */

type Row = Record<string, string | number>;
const RESULTS = path.join(__dirname, '..', 'perf-results');
const rows: Row[] = [];

function record(row: Row) {
  rows.push(row);
  fs.mkdirSync(RESULTS, { recursive: true });
  fs.appendFileSync(path.join(RESULTS, 'results.jsonl'), JSON.stringify(row) + '\n');
}

/** A phone on mid 4G with a 4x slower CPU (Chromium's DevTools protocol). */
async function throttle(page: Page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  await cdp.send('Network.emulateNetworkConditions', {
    offline: false,
    latency: 150,
    downloadThroughput: (9 * 1024 * 1024) / 8,
    uploadThroughput: (1.5 * 1024 * 1024) / 8,
  });
  await cdp.send('Emulation.setCPUThrottlingRate', { rate: 4 });
}

/** API traffic of one load: count, 429s, bytes and the last response time. */
function watchApi(page: Page) {
  const seen = { requests: 0, limited: 0, bytes: 0, lastEnd: 0 };
  const onFinished = async (req: import('@playwright/test').Request) => {
    if (!req.url().includes('/api/')) return;
    seen.requests += 1;
    const resp = await req.response().catch(() => null);
    if (resp?.status() === 429) seen.limited += 1;
    const sizes = await req.sizes().catch(() => null);
    if (sizes) seen.bytes += sizes.responseBodySize;
    const t = req.timing();
    seen.lastEnd = Math.max(seen.lastEnd, t.startTime + t.responseEnd);
  };
  page.on('requestfinished', onFinished);
  return { seen, stop: () => page.off('requestfinished', onFinished) };
}

async function loadAndMeasure(page: Page, pagePath: string, ready: (p: Page) => Promise<void>) {
  const api = watchApi(page);
  await page.goto(pagePath);
  await ready(page);
  const readyMs = await page.evaluate(() => Math.round(performance.now()));
  await page.waitForLoadState('networkidle');
  api.stop();
  const { origin, marks } = await page.evaluate(() => ({
    origin: performance.timeOrigin,
    marks: Object.fromEntries(
      performance
        .getEntriesByType('mark')
        .filter((m) => m.name.startsWith('et:'))
        .map((m) => [m.name, Math.round(m.startTime)]),
    ),
  }));
  return {
    mounted: marks['et:mounted'] ?? -1,
    auth: marks['et:auth'] ?? -1,
    ready: readyMs,
    data: api.seen.lastEnd ? Math.round(api.seen.lastEnd - origin) : -1,
    api: api.seen.requests,
    limited: api.seen.limited,
    kB: Math.round(api.seen.bytes / 1024),
  };
}

const visible = (pick: (p: Page) => import('@playwright/test').Locator, timeout = 60_000) =>
  async (p: Page) => {
    await expect(pick(p)).toBeVisible({ timeout });
  };

const PAGES: { name: string; path: string | (() => Promise<string>); ready: (p: Page) => Promise<void> }[] = [
  { name: 'my-day', path: '/tasks/my-day', ready: visible((p) => p.locator('[data-task-id]').first()) },
  { name: 'captures', path: '/tasks/captures', ready: visible((p) => p.getByRole('heading', { name: 'Captures' })) },
  { name: 'calendar', path: '/tasks/calendar', ready: visible((p) => p.locator('[data-date]').first()) },
  { name: 'dashboard', path: '/dashboard', ready: visible((p) => p.getByText(/perf project \d+/).first()) },
  { name: 'nodes', path: '/nodes', ready: visible((p) => p.getByText(/perf (article|project|area|reference) \d+/).first()) },
  { name: 'notes', path: '/notes', ready: visible((p) => p.getByText(/perf note/).first()) },
  { name: 'search', path: '/search', ready: visible((p) => p.locator('main')) },
  { name: 'data-health', path: '/data-health', ready: visible((p) => p.getByRole('heading', { name: 'Data health' })) },
  { name: 'graph', path: '/graph', ready: visible((p) => p.locator('g[data-node-id]').first(), 180_000) },
];

test.describe.configure({ mode: 'serial' });

for (const pg of PAGES) {
  test(`load ${pg.name}`, async ({ page }, info) => {
    if (info.project.name === 'phone-4g') await throttle(page);
    const target = typeof pg.path === 'string' ? pg.path : await pg.path();
    for (const visit of ['cold', 'warm'] as const) {
      const m = await loadAndMeasure(page, target, pg.ready);
      record({ project: info.project.name, page: pg.name, visit, ...m });
    }
  });
}

test('load a node page', async ({ page, request }, info) => {
  if (info.project.name === 'phone-4g') await throttle(page);
  // A busy node: an area with children, tags, notes and links.
  const hits = await (await request.get('/api/search?q=perf%20area%2020&per_page=1')).json();
  const id: string | undefined = hits?.results?.[0]?.node_id;
  test.skip(!id, 'no seeded node found (run scripts/perf/seed.sh first)');
  for (const visit of ['cold', 'warm'] as const) {
    const m = await loadAndMeasure(page, `/nodes/${id}`, visible((p) => p.getByText(/Notes on/).first()));
    record({ project: info.project.name, page: 'node', visit, ...m });
  }
});

test('actions: tick a task on My Day, quick capture', async ({ page, request }, info) => {
  if (info.project.name === 'phone-4g') await throttle(page);
  await page.goto('/tasks/my-day');
  const row = page.locator('[data-task-id]').first();
  await expect(row).toBeVisible({ timeout: 60_000 });
  await page.waitForLoadState('networkidle');

  // Tick (and untick) the first task: the requests one click sets off.
  let api = watchApi(page);
  const t0 = Date.now();
  await row.locator('button[title="Toggle done"]').click();
  await page.waitForLoadState('networkidle');
  api.stop();
  record({ project: info.project.name, page: 'my-day', visit: 'tick', ready: Date.now() - t0, api: api.seen.requests, limited: api.seen.limited, kB: Math.round(api.seen.bytes / 1024) });
  await row.locator('button[title="Toggle done"]').click();
  await page.waitForLoadState('networkidle');

  // A quick capture from My Day: what reloads that has nothing to do with it.
  api = watchApi(page);
  const t1 = Date.now();
  await page.locator('main').click({ position: { x: 5, y: 5 } });
  await page.keyboard.press('n');
  await page.getByPlaceholder(/What’s on your mind/).fill('perf capture');
  await page.keyboard.press('ControlOrMeta+Enter');
  await page.waitForLoadState('networkidle');
  api.stop();
  record({ project: info.project.name, page: 'my-day', visit: 'capture', ready: Date.now() - t1, api: api.seen.requests, limited: api.seen.limited, kB: Math.round(api.seen.bytes / 1024) });
  const caps: { id: string; body: string }[] = await (await request.get('/api/captures')).json();
  for (const c of caps.filter((c) => c.body === 'perf capture')) await request.delete(`/api/captures/${c.id}`);
});

test('graph: drag a node for one second', async ({ page }, info) => {
  test.skip(info.project.name !== 'desktop', 'mouse drag measured on desktop only');
  await page.goto('/graph');
  const node = page.locator('g[data-node-id]').first();
  await expect(node).toBeVisible({ timeout: 180_000 });
  await page.waitForLoadState('networkidle');
  const box = await node.boundingBox();
  test.skip(!box, 'node not laid out');
  const { x, y } = { x: box!.x + box!.width / 2, y: box!.y + box!.height / 2 };
  await page.mouse.move(x, y);
  await page.mouse.down();
  const t0 = Date.now();
  // 60 moves: what a one-second drag delivers at 60 Hz.
  for (let k = 1; k <= 60; k++) await page.mouse.move(x + k * 3, y + k * 2);
  const dragMs = Date.now() - t0;
  await page.mouse.up();
  record({ project: info.project.name, page: 'graph', visit: 'drag-60-moves', ready: dragMs, api: 0, limited: 0, kB: 0 });
});

test.afterAll(() => {
  if (!rows.length) return;
  const cols = ['project', 'page', 'visit', 'mounted', 'auth', 'ready', 'data', 'api', 'limited', 'kB'];
  console.log('\n' + cols.join('\t'));
  for (const r of rows) console.log(cols.map((c) => r[c] ?? '').join('\t'));
});
