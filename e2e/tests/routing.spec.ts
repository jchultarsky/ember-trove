import { test, expect } from './fixtures';

/**
 * Unknown URLs and `/admin` (#246).
 *
 * The `<Routes>` fallback used to be a `<Redirect>`, which panics in the
 * router's render: every unknown URL froze on the bare "Loading..." of the
 * auth gate. The fixtures fail a test on any `pageerror`, so a panic here
 * fails the spec, not just the URL check.
 */

test('an unknown URL goes to My Day', async ({ page }) => {
  await page.goto('/no-such-page');
  await expect(page).toHaveURL(/\/tasks\/my-day$/);
  await expect(page.getByRole('heading', { name: 'My Day' })).toBeVisible();
});

test('/admin opens the Users page', async ({ page }) => {
  // The e2e identities aren't admins; stub the user list the page loads.
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      json: { sub: 'e2e-admin', email: 'admin@example.com', name: 'Admin', roles: ['admin'] },
    }),
  );
  await page.route('**/api/admin/users/roles', (route) => route.fulfill({ json: ['admin', 'user'] }));
  await page.route('**/api/admin/users', (route) => route.fulfill({ json: [] }));
  await page.goto('/admin');
  await expect(page).toHaveURL(/\/admin\/users$/);
  await expect(page.getByRole('heading', { name: 'User Management' })).toBeVisible();
});
