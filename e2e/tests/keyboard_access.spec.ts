import { test, expect, createNode, localISO } from './fixtures';

/**
 * Rows and cells that only a mouse could open (#281, the review's
 * accessibility list). Each has a keyboard way in: a real button for its
 * title, or a focusable cell with an Enter handler.
 */

test('a node in the node list opens from the keyboard', async ({ page, request }) => {
  const title = `e2e keyboard node ${Date.now()}`;
  const node = await createNode(request, title);
  try {
    await page.goto('/nodes');
    const open = page.getByRole('button', { name: title, exact: true });
    await open.focus();
    await expect(open).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('a person opens from the keyboard', async ({ page, request }) => {
  const name = `e2e Keyboard Person ${Date.now()}`;
  const person = await (
    await request.post('/api/people', { data: { display_name: name } })
  ).json();
  try {
    await page.goto('/people');
    const open = page.getByRole('button', { name, exact: true });
    await open.focus();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(new RegExp(`/people/${person.id}$`));
  } finally {
    await request.delete(`/api/people/${person.id}`);
  }
});

test('a calendar day opens its composer from the keyboard', async ({ page }) => {
  await page.goto('/tasks/calendar');
  const cell = page.locator(`[data-date="${localISO(0)}"]`);
  await expect(cell).toHaveAttribute('role', 'button');
  await cell.focus();
  await page.keyboard.press('Enter');
  const input = cell.getByPlaceholder('Task or capture…');
  await expect(input).toBeVisible();
  // Typing in the composer is not taken for the cell's own key.
  await input.fill('a b');
  await expect(input).toHaveValue('a b');
  await page.keyboard.press('Escape');
  await expect(input).toBeHidden();
});
