import { defineConfig, devices } from '@playwright/test';

/**
 * Performance measurement (P0 of the performance phase, 2026-11-02). Not part
 * of the CI suite: it measures, it doesn't assert budgets yet.
 *
 *   KEEP_STACK=1 ./scripts/e2e.sh tests/smoke.spec.ts   # stack up
 *   scripts/perf/seed.sh 10                              # synthetic data
 *   KEEP_STACK=1 ./scripts/e2e.sh --config playwright.perf.config.ts
 *
 * Results: one JSON line per measurement in e2e/perf-results/ (git-ignored),
 * and a table on stdout. See docs/PERFORMANCE.md.
 */
export default defineConfig({
  testDir: './perf',
  testMatch: /.*\.perf\.ts$/,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: 'list',
  timeout: 240_000,
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:8003',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'] } },
    // A phone on a mid 4G link with a slower CPU: Chromium only, because the
    // throttling uses the DevTools protocol (see `throttle` in the spec).
    { name: 'phone-4g', use: { ...devices['Pixel 7'] } },
    // The engine every iPhone browser uses. No throttling available here.
    { name: 'iphone', use: { ...devices['iPhone 15'] } },
  ],
});
