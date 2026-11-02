import { test, expect, type Page } from './fixtures';

/**
 * Admin → Performance: the API's timings and the database's slowest
 * statements (performance phase, 2026-11-02).
 *
 * No bypass identity is an admin (on purpose), so the page tests stub
 * `/api/auth/me` and the two metrics endpoints, as admin_users.spec.ts
 * does. The last test hits the real API as a non-admin.
 */

const ME = '0a0a0a0a-0000-4000-8000-00000000000a';

function routeRow(route: string, count: number, total: number, p95: number, max: number) {
  return {
    route,
    count,
    total_ms: total,
    mean_ms: total / count,
    p50_ms: 1,
    p95_ms: p95,
    p99_ms: p95,
    max_ms: max,
  };
}

const METRICS = {
  version: '9.9.9',
  uptime_secs: 3_600 * 3 + 60 * 20,
  db: { pool_size: 5, pool_idle: 3 },
  counts: { nodes: 300, edges: 130, tags: 12, notes: 500, tasks: 280, attachments: 4, favorites: 9 },
  requests: { total: 1234, status_2xx: 1200, status_3xx: 4, status_4xx: 28, status_5xx: 2 },
  routes: [
    routeRow('GET /api/my-day', 40, 2_000, 60, 90),
    routeRow('GET /api/graph', 5, 1_500, 1_200, 1_400),
    routeRow('GET /api/health', 900, 300, 1, 3),
  ],
};

const STATEMENTS = {
  enabled: true,
  statements: [
    {
      query: 'SELECT n.id, n.title FROM nodes n WHERE n.owner_id = $1',
      calls: 77,
      total_ms: 120.5,
      mean_ms: 1.6,
      max_ms: 9.1,
      rows: 3000,
      blks_read: 0,
    },
  ],
};

async function stubPerformance(page: Page, statements: object = STATEMENTS) {
  const calls: string[] = [];
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { sub: ME, email: 'me@example.com', name: 'Me', roles: ['admin'] } }),
  );
  await page.route('**/api/metrics', (route) => {
    calls.push('metrics');
    return route.fulfill({ json: METRICS });
  });
  await page.route('**/api/metrics/db', (route) => {
    calls.push('db');
    return route.fulfill({ json: statements });
  });
  return calls;
}

async function gotoPerformance(page: Page) {
  await page.goto('/admin/performance');
  await expect(page.getByTestId('perf-routes')).toBeVisible({ timeout: 15_000 });
}

function routeNames(page: Page) {
  return page.getByTestId('perf-routes').locator('tbody tr td:first-child');
}

test('the page shows the summary and routes, slowest total first', async ({ page }) => {
  await stubPerformance(page);
  await gotoPerformance(page);

  const summary = page.getByTestId('perf-summary');
  await expect(summary).toContainText('9.9.9');
  await expect(summary).toContainText('3 h 20 min');
  await expect(summary).toContainText('1234');
  await expect(summary).toContainText('2 of 5 in use');
  await expect(routeNames(page)).toHaveText(['GET /api/my-day', 'GET /api/graph', 'GET /api/health']);

  // A slow p95 is flagged in the heat colour; a fast one isn't.
  const graph = page.getByTestId('perf-routes').locator('tbody tr', { hasText: 'GET /api/graph' });
  await expect(graph.locator('td').nth(4)).toHaveText('1.2 s');
  await expect(graph.locator('td').nth(4)).toHaveClass(/text-heat-max/);
  const health = page.getByTestId('perf-routes').locator('tbody tr', { hasText: 'GET /api/health' });
  await expect(health.locator('td').nth(4)).not.toHaveClass(/text-heat/);
});

test('the route table sorts by its column headers', async ({ page }) => {
  await stubPerformance(page);
  await gotoPerformance(page);
  const table = page.getByTestId('perf-routes');

  await table.getByRole('button', { name: 'p95' }).click();
  await expect(routeNames(page)).toHaveText(['GET /api/graph', 'GET /api/my-day', 'GET /api/health']);
  // Only the rows re-render: the header button keeps focus for the keyboard.
  await expect(table.getByRole('button', { name: 'p95' })).toBeFocused();
  await expect(table.locator('th', { hasText: 'p95' })).toHaveAttribute('aria-sort', 'descending');

  await table.getByRole('button', { name: 'Count' }).click();
  await expect(routeNames(page)).toHaveText(['GET /api/health', 'GET /api/my-day', 'GET /api/graph']);

  await table.getByRole('button', { name: 'Route' }).click();
  await expect(routeNames(page)).toHaveText(['GET /api/graph', 'GET /api/health', 'GET /api/my-day']);
  await expect(table.locator('th', { hasText: 'Route' })).toHaveAttribute('aria-sort', 'ascending');
});

test('statements are listed, and Refresh reads both again', async ({ page }) => {
  const calls = await stubPerformance(page);
  await gotoPerformance(page);
  await expect(page.getByTestId('perf-statements')).toContainText('WHERE n.owner_id = $1');
  await expect(page.getByTestId('perf-statements')).toContainText('77');

  const before = calls.length;
  await page.getByRole('button', { name: 'Refresh' }).click();
  await expect.poll(() => calls.slice(before).sort()).toEqual(['db', 'metrics']);
});

test('without pg_stat_statements the page says so', async ({ page }) => {
  await stubPerformance(page, { enabled: false, statements: [] });
  await gotoPerformance(page);
  await expect(page.getByText("Database statistics aren't enabled")).toBeVisible();
  await expect(page.getByTestId('perf-statements')).toHaveCount(0);
});

test('the sidebar links an admin to the page', async ({ page }) => {
  await stubPerformance(page);
  await page.goto('/tasks/my-day');
  await expect(page.locator('main')).toBeVisible();
  await page.getByRole('button', { name: 'Performance' }).click();
  await expect(page).toHaveURL(/\/admin\/performance$/);
  await expect(page.getByTestId('perf-routes')).toBeVisible();
});

test('the metrics endpoints refuse a non-admin', async ({ request }) => {
  for (const path of ['/api/metrics', '/api/metrics/db']) {
    const resp = await request.get(path);
    expect(resp.status(), path).toBe(403);
  }
});
