import { test, expect, Page } from '@playwright/test';

/**
 * Smoke suite — the flows that host-side tests structurally cannot cover
 * (WASM runtime behavior in a real browser). Both v2.21.1 hotfix bugs are
 * regression-tested here: the zombie window-listener panic and the
 * silently-dropped undo toast.
 *
 * Runs against the e2e stack (scripts/e2e.sh): auth is bypassed server-side,
 * the database starts empty and is ephemeral.
 */

/** Collect WASM panics / JS exceptions for the whole test. */
function collectPageErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (err) => errors.push(String(err)));
  return errors;
}

/** Focus the page body so single-key shortcuts aren't swallowed by inputs. */
async function focusNeutral(page: Page) {
  await page.locator('main').click({ position: { x: 5, y: 5 } });
}

test.describe('app shell', () => {
  test('loads My Day with route title', async ({ page }) => {
    const errors = collectPageErrors(page);
    await page.goto('/tasks/my-day');
    await expect(page).toHaveTitle('My Day — Ember Trove');
    await expect(page.getByRole('heading', { name: 'My Day' })).toBeVisible();
    expect(errors).toEqual([]);
  });
});

test.describe('quick capture with natural-language tokens', () => {
  test('parses tokens, previews chips, lands in the inbox', async ({ page, request }) => {
    const title = `e2e capture ${Date.now()}`;
    await page.goto('/tasks/my-day');
    await focusNeutral(page);
    await page.keyboard.press('n');

    const box = page.getByPlaceholder(/What's on your mind/);
    await expect(box).toBeVisible();
    await box.fill(`${title} tomorrow p1`);

    // Live parse preview chips.
    await expect(page.getByText(/Due (Mon|Tue|Wed|Thu|Fri|Sat|Sun)/)).toBeVisible();
    await expect(page.getByText('High priority')).toBeVisible();

    await page.keyboard.press('ControlOrMeta+Enter');
    await expect(page.locator('[role="status"]').getByText(/Captured to Inbox/)).toBeVisible();

    // The task lands de-tokenized, with the parsed priority.
    await page.goto('/tasks/inbox');
    const row = page.locator('[data-task-id]', { hasText: title });
    await expect(row).toBeVisible();
    await expect(row.getByRole('img', { name: 'High priority' })).toBeVisible();

    // Clean up — later specs (triage) assume they control the inbox.
    const inbox = await (await request.get('/api/tasks/inbox')).json();
    const created = inbox.find((t: { title: string }) => t.title === title);
    if (created) await request.delete(`/api/tasks/${created.id}`);
  });
});

test.describe('soft delete with undo', () => {
  test('delete shows an undo toast; undo restores the task', async ({ page }) => {
    const title = `e2e undo ${Date.now()}`;
    await page.goto('/tasks/inbox');

    // Create via the inline form.
    await page.getByPlaceholder('Task title…').fill(title);
    await page.keyboard.press('Enter');
    const row = page.locator('[data-task-id]', { hasText: title });
    await expect(row).toBeVisible();

    // Delete → undo toast (v2.21.1 regression: this toast was silently
    // dropped because use_context has no owner after .await).
    await row.getByRole('button', { name: 'Delete' }).click();
    const toast = page.locator('[role="status"]');
    await expect(toast.getByText('Task deleted')).toBeVisible();
    await expect(row).not.toBeVisible();

    // Undo → task returns.
    await toast.getByRole('button', { name: 'Undo' }).click();
    await expect(page.locator('[data-task-id]', { hasText: title })).toBeVisible();

    // Clean up (delete again, let the toast lapse).
    await page.locator('[data-task-id]', { hasText: title })
      .getByRole('button', { name: 'Delete' }).click();
  });
});

test.describe('keyboard listener lifecycle', () => {
  test('no WASM panic on keypress after leaving My Day', async ({ page }) => {
    // v2.21.1 regression: MyDayView leaked its window keydown listener on
    // unmount; the zombie closure read disposed signals on the next keypress,
    // panicked, and poisoned all event dispatch.
    const errors = collectPageErrors(page);

    await page.goto('/tasks/my-day');
    await expect(page.getByRole('heading', { name: 'My Day' })).toBeVisible();
    await page.getByRole('tab', { name: 'Inbox' }).click();
    await expect(page).toHaveTitle('Inbox — Ember Trove');

    await focusNeutral(page);
    await page.keyboard.press('j');
    await page.keyboard.press('x');
    // Event dispatch must still be alive: `n` opens quick capture.
    await page.keyboard.press('n');
    await expect(page.getByPlaceholder(/What's on your mind/)).toBeVisible();
    await page.keyboard.press('Escape');

    expect(errors).toEqual([]);
  });
});

test.describe('editor autosave', () => {
  test('debounced autosave persists without manual save', async ({ page, request }) => {
    const title = `e2e autosave ${Date.now()}`;
    const created = await request.post('/api/nodes', {
      data: {
        title,
        node_type: 'article',
        body: 'initial body',
        metadata: {},
        status: 'draft',
      },
    });
    expect(created.ok()).toBeTruthy();
    const node = await created.json();

    try {
      await page.goto(`/nodes/${node.id}/edit`);
      const editor = page.getByPlaceholder(/Write in Markdown/);
      await expect(editor).toHaveValue('initial body');

      await editor.click();
      await editor.press('End');
      await editor.pressSequentially(' plus autosaved text');
      await expect(page.getByText('Unsaved changes…')).toBeVisible();
      // Debounce is 2s; allow for the PATCH round-trip.
      await expect(page.getByText('Saved', { exact: true })).toBeVisible({ timeout: 10_000 });

      // Server state — not just the indicator.
      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('initial body plus autosaved text');
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });
});
