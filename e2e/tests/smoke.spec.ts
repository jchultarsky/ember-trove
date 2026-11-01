import { test, expect, type Page } from './fixtures';

/**
 * Smoke suite — the flows that host-side tests structurally cannot cover
 * (WASM runtime behavior in a real browser). Both v2.21.1 hotfix bugs are
 * regression-tested here: the zombie window-listener panic and the
 * silently-dropped undo toast.
 *
 * Runs against the e2e stack (scripts/e2e.sh): auth is bypassed server-side,
 * the database starts empty and is ephemeral.
 */

/** Focus the page body so single-key shortcuts aren't swallowed by inputs. */
async function focusNeutral(page: Page) {
  await page.locator('main').click({ position: { x: 5, y: 5 } });
}

test.describe('app shell', () => {
  test('loads My Day with route title', async ({ page }) => {
    await page.goto('/tasks/my-day');
    await expect(page).toHaveTitle('My Day — Ember Trove');
    await expect(page.getByRole('heading', { name: 'My Day' })).toBeVisible();
  });

  // #173: a caption marks the sidebar's Tasks / Notes / People as global,
  // not the node page's sections of the same names. CSS uppercases it; the
  // DOM text is "Workspace". Collapsing the sidebar hides it.
  test('captions the global sidebar entries; hidden when collapsed', async ({ page }) => {
    await page.goto('/tasks/my-day');
    const caption = page.getByTestId('sidebar-caption-workspace');
    await expect(caption).toHaveText('Workspace');
    await expect(caption).toBeVisible();

    await page.locator('button[title="Collapse sidebar"]').click();
    await expect(caption).toHaveCount(0);
    await page.locator('button[title="Expand sidebar"]').click();
    await expect(caption).toBeVisible();
  });

  // The suite must run under an eval-free CSP, or the `cspViolations`
  // fixture proves nothing. WebAssembly needs only 'wasm-unsafe-eval'.
  test('is served with an eval-free CSP', async ({ page }) => {
    const resp = await page.goto('/tasks/my-day');
    const csp = resp?.headers()['content-security-policy'] ?? '';
    expect(csp).toContain("'wasm-unsafe-eval'");
    expect(csp).not.toContain("'unsafe-eval'");
    await expect(page.getByRole('heading', { name: 'My Day' })).toBeVisible();
  });
});

test.describe('quick capture', () => {
  test('lands a capture in the inbox, words kept as typed (#234)', async ({ page, request }) => {
    const text = `e2e capture ${Date.now()} tomorrow p1`;
    await page.goto('/tasks/my-day');
    await focusNeutral(page);
    await page.keyboard.press('n');

    const box = page.getByPlaceholder(/What’s on your mind/);
    await expect(box).toBeVisible();
    await box.fill(text);
    // A capture isn't classified yet: no date or priority chips.
    await expect(page.getByText('High priority')).toHaveCount(0);

    await page.keyboard.press('ControlOrMeta+Enter');
    await expect(page.locator('[role="status"]').getByText(/Captured to Inbox/)).toBeVisible();

    // A capture, not a task: "tomorrow p1" is still in its text.
    const captures: { id: string; body: string }[] = await (await request.get('/api/captures')).json();
    const created = captures.find((c) => c.body === text);
    expect(created).toBeTruthy();
    const inbox: { title: string }[] = await (await request.get('/api/tasks/inbox')).json();
    expect(inbox.some((t) => t.title.includes(text))).toBe(false);
    await request.delete(`/api/captures/${created!.id}`);
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
  test('no WASM panic on keypress after leaving My Day', async ({ page, pageErrors }) => {
    // v2.21.1 regression: MyDayView leaked its window keydown listener on
    // unmount; the zombie closure read disposed signals on the next keypress,
    // panicked, and poisoned all event dispatch.
    await page.goto('/tasks/my-day');
    await expect(page.getByRole('heading', { name: 'My Day' })).toBeVisible();
    await page.getByRole('tab', { name: 'Inbox' }).click();
    await expect(page).toHaveTitle('Inbox — Ember Trove');

    await focusNeutral(page);
    await page.keyboard.press('j');
    await page.keyboard.press('x');
    // Event dispatch must still be alive: `n` opens quick capture.
    await page.keyboard.press('n');
    await expect(page.getByPlaceholder(/What.s on your mind/)).toBeVisible();
    await page.keyboard.press('Escape');
    // (The fixture also fails any test on a page error; explicit here.)
    expect(pageErrors).toEqual([]);
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
      // Persistent label carries the local save time ("Saved · 4:02 PM" or
      // "Saved · 16:02", per locale) — #98 item 6.
      await expect(page.getByText(/^Saved · \d{1,2}:\d{2}/)).toBeVisible({ timeout: 10_000 });

      // Server state — not just the indicator.
      const fetched = await request.get(`/api/nodes/${node.id}`);
      expect((await fetched.json()).body).toBe('initial body plus autosaved text');
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
    }
  });
});
