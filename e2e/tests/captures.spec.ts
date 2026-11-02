import { test, expect, createNode, type APIRequestContext, type Page } from './fixtures';

/**
 * Captures (#234, ONTOLOGY §9.1): unclassified input on the Captures wall
 * (`/tasks/captures`, the Inbox until S4c, #240). Quick capture lands one;
 * it can be reworded and deliberately deleted (with Undo). Captures are
 * owner-only, with no admin bypass. Every capture made
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
  await expect(page.locator('[role="status"]').getByText(/Added to Captures/)).toBeVisible();
}

test('a capture lands on the wall, is reworded, deleted, and comes back with Undo', async ({
  page,
}) => {
  const text = `${PREFIX}${Date.now()} call the bank`;
  await captureByKey(page, text);

  await page.goto('/tasks/captures');
  const row = page.getByTestId('captures').getByTestId('capture').filter({ hasText: text });
  await expect(row).toBeVisible();
  // #240: a fresh capture's age tag reads "today", with the words for a
  // screen reader beside it.
  await expect(row.getByTestId('capture-age')).toHaveText('today');
  await expect(row.getByText('captured today')).toBeAttached();

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

  // Nor does their wall show it.
  await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
  await page.goto('/tasks/captures');
  await expect(page.getByRole('heading', { name: 'Captures' })).toBeVisible();
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
    const compose = page.getByPlaceholder('Write a note…  (Ctrl+Enter to post)');
    await compose.fill(`${PREFIX}on a node ${stamp}`);
    await expect(post).toBeDisabled();
    // The keyboard path says why, and doesn't lock the box (review of #234:
    // it used to stay on "Posting…" for good).
    await compose.press('ControlOrMeta+Enter');
    await expect(page.getByText('Pick a node for this note, or capture it instead.')).toBeVisible();
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

// ── The Captures view (S4c, #240) ───────────────────────────────────────────

test('the old Inbox URLs land on Captures, the share marker included', async ({ page }) => {
  // Installed service workers still send a share to /tasks/inbox?captured=1.
  await page.goto('/tasks/inbox?captured=1');
  await expect(page).toHaveURL(/\/tasks\/captures$/);
  await expect(page.locator('[role="status"]').getByText('Added to Captures')).toBeVisible();
  await expect(page).toHaveTitle('Captures — Ember Trove');

  await page.goto('/inbox');
  await expect(page).toHaveURL(/\/tasks\/captures$/);
  await expect(page.getByRole('tab', { name: 'Captures' })).toHaveAttribute('aria-selected', 'true');
});

test('an empty wall says so; a task with no Project is not a capture', async ({
  page,
  otherRequest,
}) => {
  // The second identity's wall, emptied (it has nothing else to keep).
  const theirs: { id: string }[] = await (await otherRequest.get('/api/captures')).json();
  for (const c of theirs) await otherRequest.delete(`/api/captures/${c.id}`);
  // A task with no Project: Data health lists it, the wall doesn't.
  const title = `${PREFIX}${Date.now()} loose task`;
  const task = await (await otherRequest.post('/api/tasks', { data: { title } })).json();
  try {
    await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
    await page.goto('/tasks/captures');
    await expect(page.getByText('Nothing captured')).toBeVisible();
    await expect(page.getByText(title)).toHaveCount(0);
  } finally {
    await page.setExtraHTTPHeaders({});
    await otherRequest.delete(`/api/tasks/${task.id}`);
  }
});

test('the Notes box hands loose text to Captures', async ({ page, request }) => {
  // Muse's v3.5.0 friction item (#234): "press n" opened the composer empty.
  const text = `${PREFIX}${Date.now()} from the notes box`;
  await page.goto('/notes');
  const compose = page.getByPlaceholder('Write a note…  (Ctrl+Enter to post)');
  const captureIt = page.getByRole('button', { name: 'Capture it' });
  await expect(captureIt).toBeDisabled();
  await compose.fill(text);
  await captureIt.click();

  const toast = page.locator('[role="status"]');
  await expect(toast.getByText('Added to Captures')).toBeVisible();
  await expect(compose).toHaveValue('');
  const mine: { body: string }[] = await (await request.get('/api/captures')).json();
  expect(mine.filter((c) => c.body === text)).toHaveLength(1);

  // The toast's Open goes to the wall.
  await toast.getByRole('button', { name: 'Open' }).click();
  await expect(page).toHaveURL(/\/tasks\/captures$/);
  await expect(page.getByTestId('capture').filter({ hasText: text })).toBeVisible();
});

// ── Converting a capture (S4b, §9.1) ────────────────────────────────────────

async function convertDialog(page: Page, text: string) {
  await page.goto('/tasks/captures');
  const row = page.getByTestId('capture').filter({ hasText: text });
  await row.getByRole('button', { name: 'Convert capture' }).click();
  const dialog = page.getByRole('dialog', { name: 'Convert capture' });
  await expect(dialog).toBeVisible();
  return { row, dialog };
}

async function pick(page: Page, combobox: string, title: string) {
  await page.getByRole('combobox', { name: combobox }).fill(title);
  await page.getByRole('option', { name: new RegExp(title) }).click();
}

test('a capture becomes a node filed into its home, then opens', async ({ page, request }) => {
  const stamp = Date.now();
  const text = `${PREFIX}${stamp} garden plan\nbeds by the fence`;
  await request.post('/api/captures', { data: { body: text } });
  const area = await createNode(request, `${PREFIX}area ${stamp}`, { node_type: 'area' });
  let made: string | undefined;
  try {
    const { row, dialog } = await convertDialog(page, `${PREFIX}${stamp} garden plan`);
    // The first line is the title, the rest the body.
    await expect(dialog.getByRole('textbox', { name: 'Title' })).toHaveValue(`${PREFIX}${stamp} garden plan`);
    await expect(dialog.getByRole('textbox', { name: 'Body' })).toHaveValue('beds by the fence');
    await pick(page, 'Choose a home (Area)…', `${PREFIX}area ${stamp}`);
    await dialog.getByRole('button', { name: 'Convert' }).click();

    await expect(row).toHaveCount(0);
    const toast = page.locator('[role="status"]').filter({ hasText: 'Made a new Article' });
    await toast.getByRole('button', { name: 'Open' }).click();
    await expect(page).toHaveURL(/\/nodes\/[0-9a-f-]{36}$/);
    made = page.url().split('/').pop();
    const home = await (await request.get(`/api/nodes/${made}/home`)).json();
    expect(home.home.node.id).toBe(area.id);
  } finally {
    if (made) await request.delete(`/api/nodes/${made}`);
    await request.delete(`/api/nodes/${area.id}`);
  }
});

test('a capture becomes a note on a node, or a task in a Project', async ({ page, request }) => {
  const stamp = Date.now();
  const host = await createNode(request, `${PREFIX}host ${stamp}`);
  const project = await createNode(request, `${PREFIX}proj ${stamp}`, { node_type: 'project' });
  try {
    const note = `${PREFIX}${stamp} met Ana about the lease`;
    await request.post('/api/captures', { data: { body: note } });
    let { row, dialog } = await convertDialog(page, note);
    await dialog.getByRole('button', { name: 'Note', exact: true }).click();
    await pick(page, 'Note on a node…', `${PREFIX}host ${stamp}`);
    await dialog.getByRole('button', { name: 'Convert' }).click();
    await expect(row).toHaveCount(0);
    const notes: { body: string }[] = await (await request.get(`/api/nodes/${host.id}/notes`)).json();
    expect(notes.map((n) => n.body)).toContain(note);

    const task = `${PREFIX}${stamp} call the bank`;
    await request.post('/api/captures', { data: { body: task } });
    ({ row, dialog } = await convertDialog(page, task));
    await dialog.getByRole('button', { name: 'Task', exact: true }).click();
    const convert = dialog.getByRole('button', { name: 'Convert' });
    await expect(convert).toBeDisabled(); // no Project yet (§9.3)
    await pick(page, 'Task in a Project…', `${PREFIX}proj ${stamp}`);
    await dialog.getByLabel('Due date').fill('2026-11-07');
    await convert.click();
    await expect(row).toHaveCount(0);
    await expect(page.locator('[role="status"]').getByText(/Added a task to/)).toBeVisible();
    const tasks: { title: string; due_date: string }[] = await (
      await request.get(`/api/nodes/${project.id}/tasks`)
    ).json();
    expect(tasks.find((t) => t.title === task)?.due_date).toBe('2026-11-07');
  } finally {
    await request.delete(`/api/nodes/${host.id}`);
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a long capture must be reworded to become a task', async ({ page, request }) => {
  const stamp = Date.now();
  const project = await createNode(request, `${PREFIX}proj long ${stamp}`, { node_type: 'project' });
  const text = `${PREFIX}${stamp} ${'word '.repeat(110)}`;
  await request.post('/api/captures', { data: { body: text } });
  try {
    const { row, dialog } = await convertDialog(page, `${PREFIX}${stamp}`);
    await dialog.getByRole('button', { name: 'Task', exact: true }).click();
    await pick(page, 'Task in a Project…', `${PREFIX}proj long ${stamp}`);
    const convert = dialog.getByRole('button', { name: 'Convert' });
    // Never cut: the whole text is there, over the limit, so Convert waits.
    await expect(dialog.getByTestId('title-counter')).toHaveText(/\/ 500/);
    await expect(convert).toBeDisabled();
    await dialog.getByRole('textbox', { name: 'Task title' }).fill(`${PREFIX}${stamp} the short version`);
    await expect(convert).toBeEnabled();
    await convert.click();
    await expect(row).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('a conversion is the owner’s, and a task needs a Project', async ({ request, otherRequest }) => {
  const stamp = Date.now();
  const made = await request.post('/api/captures', { data: { body: `${PREFIX}${stamp} mine` } });
  const id = (await made.json()).id as string;
  const article = await createNode(request, `${PREFIX}article ${stamp}`);
  try {
    // Another account: 404, as if it didn't exist, and it stays.
    const theirs = await otherRequest.post(`/api/captures/${id}/convert`, {
      data: { into: 'node', node_type: 'article', title: `${PREFIX}stolen ${stamp}` },
    });
    expect(theirs.status()).toBe(404);
    // A Task on an Article: 422 (§9.3), and the capture stays.
    const onArticle = await request.post(`/api/captures/${id}/convert`, {
      data: { into: 'task', node_id: article.id, title: 'x' },
    });
    expect(onArticle.status()).toBe(422);
    const mine: { id: string }[] = await (await request.get('/api/captures')).json();
    expect(mine.some((c) => c.id === id)).toBe(true);
  } finally {
    await request.delete(`/api/nodes/${article.id}`);
  }
});

test('the convert dialog works from the keyboard', async ({ page, request }) => {
  const stamp = Date.now();
  const host = await createNode(request, `${PREFIX}kbd host ${stamp}`);
  const text = `${PREFIX}${stamp} by keyboard`;
  await request.post('/api/captures', { data: { body: text } });
  try {
    // Focus starts inside: Escape closes, and the capture stays (review of
    // S4b: focus used to stay on the row, so Escape did nothing).
    let { row, dialog } = await convertDialog(page, text);
    await expect(dialog.getByRole('textbox', { name: 'Title' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(row).toBeVisible();

    // A keyboard pick keeps focus in the dialog, so Ctrl+Enter converts.
    ({ row, dialog } = await convertDialog(page, text));
    await dialog.getByRole('button', { name: 'Note', exact: true }).click();
    const picker = page.getByRole('combobox', { name: 'Note on a node…' });
    await picker.fill(`${PREFIX}kbd host ${stamp}`);
    await expect(
      page.getByRole('listbox').getByRole('option', { selected: true }),
    ).toContainText(`kbd host ${stamp}`);
    await picker.press('Enter');
    await expect(dialog.getByRole('button', { name: 'Convert' })).toBeFocused();
    await page.keyboard.press('ControlOrMeta+Enter');
    await expect(row).toHaveCount(0);
    const notes: { body: string }[] = await (await request.get(`/api/nodes/${host.id}/notes`)).json();
    expect(notes.map((n) => n.body)).toContain(text);
  } finally {
    await request.delete(`/api/nodes/${host.id}`);
  }
});
