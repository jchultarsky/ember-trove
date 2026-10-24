import { test, expect } from './fixtures';

/**
 * ModalShell — the chrome every modal shares. The change-password dialog was
 * the one hand-rolled modal with no dialog role, no Escape, and no focus
 * trap; it now gets all three from the shell, and it forgets typed
 * passwords on close.
 */

test('change-password dialog: role, focus, Tab trap, Escape, cleared on reopen', async ({
  page,
}) => {
  await page.goto('/tasks/my-day');
  // Accessible name includes the glyph text (e2e.md).
  const open = page.getByRole('button', { name: 'lock_reset Change password' });
  await open.click();

  const dialog = page.getByRole('dialog', { name: 'Change password' });
  await expect(dialog).toBeVisible();
  const current = dialog.locator('input[autocomplete="current-password"]');
  await expect(current).toBeFocused();
  await current.fill('not-a-real-password');

  // Tab cycles inside the dialog, however far it goes.
  for (let i = 0; i < 8; i++) await page.keyboard.press('Tab');
  expect(
    await dialog.evaluate((el) => el.contains(document.activeElement)),
  ).toBe(true);

  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  // Focus returns to the trigger.
  await expect(open).toBeFocused();

  await open.click();
  await expect(current).toHaveValue('');
});

test('backdrop click closes a modal (help)', async ({ page }) => {
  await page.goto('/tasks/my-day');
  await page.locator('main').click({ position: { x: 5, y: 5 } });
  await page.keyboard.press('?');
  const dialog = page.getByRole('dialog', { name: 'Help and shortcuts' });
  await expect(dialog).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(dialog).toBeHidden();
});

test('the n that opens quick capture is not typed into it', async ({ page }) => {
  await page.goto('/tasks/my-day');
  await page.locator('main').click({ position: { x: 5, y: 5 } });
  await page.keyboard.press('n');
  const box = page.getByRole('dialog', { name: 'Quick capture' }).locator('textarea');
  await expect(box).toBeFocused();
  await expect(box).toHaveValue('');
});
