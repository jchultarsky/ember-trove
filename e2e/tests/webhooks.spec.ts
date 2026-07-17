import { test, expect, Page } from '@playwright/test';

/**
 * Webhooks management view (/webhooks) — the UI shipped for the previously
 * headless webhooks backend. Also regression-tests two server behaviors
 * through the UI:
 *   - SSRF validation errors surface to the user (422 → error toast)
 *   - toggling Active does NOT wipe a stored secret (the UpdateWebhookRequest
 *     PATCH-semantics fix: absent field = keep)
 */

function collectPageErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(String(err)));
  return errors;
}

test.describe('webhooks view', () => {
  test('create, toggle (secret survives), and delete a webhook', async ({ page, request }) => {
    const errors = collectPageErrors(page);
    const url = `https://example.com/hooks/e2e-${Date.now()}`;
    let created: { id: string } | undefined;
    try {
      await page.goto('/webhooks');
      await expect(page).toHaveTitle('Webhooks — Ember Trove');

      // Create with a secret, subscribed to all events by default.
      await page.getByRole('button', { name: 'New Webhook' }).click();
      await page.getByLabel('Endpoint URL (HTTPS)').fill(url);
      await page.getByLabel(/^Secret/).fill('e2e-signing-secret');
      await page.getByRole('button', { name: 'Save' }).click();
      await expect(page.locator('[role="status"]').getByText('Webhook created')).toBeVisible();

      const row = page.locator('[data-webhook-id]', { hasText: url });
      await expect(row).toBeVisible();
      await expect(row.getByText('Active')).toBeVisible();
      await expect(row.getByText(/signed/)).toBeVisible();

      // Server state: created, active, secret stored (masked in the response).
      const hooks = await (await request.get('/api/webhooks')).json();
      created = hooks.find((h: { url: string }) => h.url === url);
      expect(created).toBeTruthy();

      // Toggle to Paused — the secret must survive (absent field = keep).
      await row.getByRole('button', { name: 'Active' }).click();
      await expect(row.getByText('Paused')).toBeVisible();
      const after = (await (await request.get('/api/webhooks')).json())
        .find((h: { url: string }) => h.url === url);
      expect(after.is_active).toBe(false);
      expect(after.secret).toBeTruthy(); // masked, but present — not wiped

      // Delete through the confirm modal.
      await row.getByRole('button', { name: 'Delete' }).click();
      await page.getByRole('button', { name: 'Delete', exact: true }).last().click();
      await expect(page.locator('[role="status"]').getByText('Webhook deleted')).toBeVisible();
      await expect(row).not.toBeVisible();
      created = undefined;

      expect(errors).toEqual([]);
    } finally {
      if (created) await request.delete(`/api/webhooks/${created.id}`);
    }
  });

  test('SSRF-blocked URL surfaces the server validation error', async ({ page, request }) => {
    const errors = collectPageErrors(page);
    await page.goto('/webhooks');
    await page.getByRole('button', { name: 'New Webhook' }).click();
    await page.getByLabel('Endpoint URL (HTTPS)').fill('https://10.0.0.5/internal');
    await page.getByRole('button', { name: 'Save' }).click();

    // 422 from validate_webhook_url → error toast with the server's message.
    await expect(
      page.locator('[role="status"]').getByText(/private networks/),
    ).toBeVisible();

    // Nothing persisted.
    const hooks = await (await request.get('/api/webhooks')).json();
    expect(hooks.find((h: { url: string }) => h.url.includes('10.0.0.5'))).toBeFalsy();
    expect(errors).toEqual([]);
  });
});
