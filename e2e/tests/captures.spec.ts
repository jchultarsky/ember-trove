import { test, expect, createNode, type APIRequestContext, type Page } from './fixtures';

/**
 * Captures (#234, ONTOLOGY §9.1): unclassified input in the Inbox. Quick
 * capture lands one; it can be reworded and deliberately deleted (with
 * Undo). Captures are owner-only, with no admin bypass. Every capture made
 * here starts with PREFIX, and both identities' are wiped before AND after
 * each test (retry-safety, .claude/rules/e2e.md).
 */

const PREFIX = 'e2e cap ';

async function wipe(request: APIRequestContext) {
  const mine: { id: string; body: string }[] = await (await request.get('/api/captures')).json();
  for (const c of mine.filter((c) => c.body.includes(PREFIX))) {
    await request.delete(`/api/captures/${c.id}`);
  }
}

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipe(otherRequest);
});

async function captureByKey(page: Page, text: string) {
  await page.goto('/tasks/my-day');
  await expect(page.locator('main')).toBeVisible();
  await page.locator('main').click();
  await page.keyboard.press('n');
  const box = page.getByPlaceholder(/What’s on your mind/);
  await expect(box).toBeFocused();
  await box.fill(text);
  await page.keyboard.press('ControlOrMeta+Enter');
  await expect(page.locator('[role="status"]').getByText(/Captured to Inbox/)).toBeVisible();
}

test('a capture lands in the Inbox, is reworded, deleted, and comes back with Undo', async ({
  page,
}) => {
  const text = `${PREFIX}${Date.now()} call the bank`;
  await captureByKey(page, text);

  await page.goto('/tasks/inbox');
  const row = page.getByTestId('captures').getByTestId('capture').filter({ hasText: text });
  await expect(row).toBeVisible();

  await row.getByRole('button', { name: 'Edit capture' }).click();
  const field = page.getByRole('textbox', { name: 'Capture text' });
  await field.fill(`${text} about the card`);
  await page.getByRole('button', { name: 'Save capture' }).click();
  const reworded = page.getByTestId('capture').filter({ hasText: `${text} about the card` });
  await expect(reworded).toBeVisible();

  await reworded.getByRole('button', { name: 'Delete capture' }).click();
  await expect(reworded).toHaveCount(0);
  await page.locator('[role="status"]').getByRole('button', { name: 'Undo' }).click();
  await expect(reworded).toBeVisible();
});

test('a capture is its owner’s alone', async ({ page, request, otherRequest }) => {
  const text = `${PREFIX}${Date.now()} private thought`;
  const made = await request.post('/api/captures', { data: { body: text } });
  expect(made.status()).toBe(201);
  const id = (await made.json()).id as string;

  // The other identity can't list, change, delete or restore it: 404, as
  // if it didn't exist.
  const theirs: { body: string }[] = await (await otherRequest.get('/api/captures')).json();
  expect(theirs.some((c) => c.body === text)).toBe(false);
  expect((await otherRequest.patch(`/api/captures/${id}`, { data: { body: 'x' } })).status()).toBe(404);
  expect((await otherRequest.delete(`/api/captures/${id}`)).status()).toBe(404);
  expect((await otherRequest.post(`/api/captures/${id}/restore`)).status()).toBe(404);

  // Nor does their Inbox show it.
  await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
  await page.goto('/tasks/inbox');
  await expect(page.getByRole('heading', { name: 'Inbox' })).toBeVisible();
  await expect(page.getByText(text)).toHaveCount(0);
  await page.setExtraHTTPHeaders({});
});

test('the home-screen shortcut opens quick capture', async ({ page }) => {
  await page.goto('/?capture=1');
  await expect(page.getByRole('dialog', { name: 'Quick capture' })).toBeVisible();
  await expect(page).not.toHaveURL(/capture=1/);
});

test('a share that failed says so', async ({ page }) => {
  await page.goto('/?capture=failed&reason=auth');
  await expect(page.locator('[role="status"]').getByText(/sign in, then share it again/)).toBeVisible();
  await expect(page).not.toHaveURL(/capture=failed/);
});

test('a note needs a node; a loose thought is a capture', async ({ page, request }) => {
  // The API refuses a note without one (422).
  expect((await request.post('/api/notes', { data: { body: `${PREFIX}loose` } })).status()).toBe(422);

  const stamp = Date.now();
  const node = await createNode(request, `${PREFIX}note host ${stamp}`);
  try {
    await page.goto('/notes');
    const post = page.getByRole('button', { name: /Post/ });
    await page.getByPlaceholder('Write a note…  (Ctrl+Enter to post)').fill(`${PREFIX}on a node ${stamp}`);
    await expect(post).toBeDisabled();
    const picker = page.getByRole('combobox', { name: 'Note on a node…' });
    await picker.fill(`${PREFIX}note host ${stamp}`);
    await page.getByRole('option', { name: new RegExp(`note host ${stamp}`) }).click();
    await expect(post).toBeEnabled();
    await post.click();
    await expect.poll(async () =>
      ((await (await request.get(`/api/nodes/${node.id}/notes`)).json()) as { body: string }[]).map(
        (n) => n.body,
      ),
    ).toContain(`${PREFIX}on a node ${stamp}`);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});
