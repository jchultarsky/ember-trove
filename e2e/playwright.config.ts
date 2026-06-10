import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config for the Ember Trove smoke suite.
 *
 * The app stack must already be running (scripts/e2e.sh brings it up):
 * UI + API behind nginx on E2E_BASE_URL (default http://localhost:8003),
 * with the api built `--features e2e-bypass` and E2E_AUTH_BYPASS=1 so no
 * Cognito session is needed.
 */
export default defineConfig({
  testDir: './tests',
  // The suite mutates one shared backend database — keep tests sequential.
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  timeout: 30_000,
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:8003',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
