import { test, expect, createNode, type Page, type APIRequestContext } from './fixtures';

/**
 * The shared node picker as a combobox (#126): text filters, only an option
 * commits, the committed node is a chip. Exercised through the Connections
 * "add edge" form (which used to treat typed text as the target UUID) and
 * the Inbox add-task form (attach), which must behave identically.
 *
 * Node titles are unique per attempt (`Date.now()`); every created node is
 * deleted in `finally`, so retries never see leftovers.
 */

type Edge = { target_id: string };

async function openAddEdge(page: Page, nodeId: string) {
  await page.goto(`/nodes/${nodeId}`);
  await page.getByRole('button', { name: /Connections/ }).click();
  await page.locator('button[title="Add Edge"]').click();
  return {
    // Name, not just the role: a <select> is a combobox too.
    combo: page.getByRole('combobox', { name: 'Search for a node…' }),
    listbox: page.getByRole('listbox'),
    chip: page.getByTestId('node-picker-chip'),
    confirm: page.locator('button[title="Add edge"]'),
  };
}

async function edgeTargets(request: APIRequestContext, nodeId: string): Promise<string[]> {
  const edges: Edge[] = await (await request.get(`/api/nodes/${nodeId}/edges`)).json();
  return edges.map((e) => e.target_id);
}

test('the picker matches titles as you type, never body mentions', async ({ page, request }) => {
  // Regression (#126 follow-up, reproduced on v2.33.0 data): the pickers
  // used the full-text search, which matches bodies and whole words only.
  // "Physna" listed the alphabetically-first nodes that MENTION it (the node
  // titled "Physna" fell off the 8-row page), and "Phy" listed nothing — so
  // typing seemed to do nothing and the wanted node couldn't be picked.
  const stamp = Date.now();
  const word = `Zorbix${stamp}`;
  const source = await createNode(request, `e2e mention source ${stamp}`);
  const mentions = [];
  for (let i = 0; i < 9; i++) {
    mentions.push(
      await createNode(request, `aa mention ${i} ${stamp}`, { body: `Met with ${word} today.` }),
    );
  }
  const exact = await createNode(request, word);
  const prefixed = await createNode(request, `${word} roadmap`);
  try {
    const { combo, listbox, chip } = await openAddEdge(page, source.id);
    // A partial word, typed key by key like a person.
    await combo.pressSequentially(word.slice(0, 9), { delay: 40 });
    const options = listbox.getByRole('option');
    await expect(options).toHaveCount(2);
    await expect(listbox.getByRole('option', { name: /aa mention/ })).toHaveCount(0);
    // The full title: the exact match is first and highlighted.
    await combo.pressSequentially(word.slice(9), { delay: 20 });
    await expect(options.first()).toContainText(word);
    await expect(options.first()).toHaveAttribute('aria-selected', 'true');
    await expect(options.first()).not.toContainText('roadmap');
    await options.nth(1).click();
    await expect(chip).toContainText(`${word} roadmap`);
  } finally {
    for (const n of [source, exact, prefixed, ...mentions]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('an exact title + Enter commits; Enter again creates the edge', async ({ page, request }) => {
  const stamp = Date.now();
  const source = await createNode(request, `e2e pick source ${stamp}`);
  const target = await createNode(request, `e2e pick target ${stamp}`);
  try {
    const { combo, chip, confirm } = await openAddEdge(page, source.id);
    // Nothing committed yet → ✓ is disabled; text alone never enables it.
    await expect(confirm).toBeDisabled();
    // Enter straight after typing — before the debounced search has
    // answered — still resolves the unique exact title.
    await combo.fill(`e2e pick target ${stamp}`);
    await combo.press('Enter');
    await expect(chip).toContainText(`e2e pick target ${stamp}`);
    await expect(confirm).toBeEnabled();
    await expect(confirm).toBeFocused();
    await page.keyboard.press('Enter');
    await expect.poll(() => edgeTargets(request, source.id)).toContain(target.id);
    // Regression: the typed text used to be parsed as the target UUID.
    await expect(page.getByText('Invalid target node UUID')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${source.id}`);
    await request.delete(`/api/nodes/${target.id}`);
  }
});

test('arrows move the active option and Enter commits it; a press does the same', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const source = await createNode(request, `e2e arrow source ${stamp}`);
  const one = await createNode(request, `e2e arrow ${stamp} one`);
  const two = await createNode(request, `e2e arrow ${stamp} two`);
  try {
    const { combo, listbox, chip, confirm } = await openAddEdge(page, source.id);
    await combo.fill(`e2e arrow ${stamp}`);
    await expect(listbox.getByRole('option')).toHaveCount(2);
    // The page's own node is never offered as a target.
    await expect(listbox.getByRole('option', { name: /e2e arrow source/ })).toHaveCount(0);

    const first = listbox.getByRole('option').nth(0);
    const second = listbox.getByRole('option').nth(1);
    await expect(first).toHaveAttribute('aria-selected', 'true'); // autohighlight
    await combo.press('ArrowDown');
    await expect(second).toHaveAttribute('aria-selected', 'true');
    await expect(combo).toHaveAttribute('aria-activedescendant', (await second.getAttribute('id'))!);
    await combo.press('Enter');
    await expect(chip).toContainText(`e2e arrow ${stamp} two`);
    await confirm.click();
    await expect.poll(() => edgeTargets(request, source.id)).toContain(two.id);

    // Mouse path: same committed state, same result.
    await page.locator('button[title="Add Edge"]').click();
    await combo.fill(`e2e arrow ${stamp} one`);
    await listbox.getByRole('option', { name: new RegExp(`e2e arrow ${stamp} one`) }).click();
    await expect(chip).toContainText(`e2e arrow ${stamp} one`);
    await expect(confirm).toBeEnabled();
    await confirm.click();
    await expect.poll(() => edgeTargets(request, source.id)).toContain(one.id);
  } finally {
    for (const n of [source, one, two]) await request.delete(`/api/nodes/${n.id}`);
  }
});

test('same-titled nodes are both listed and never committed on a guess', async ({
  page,
  request,
}) => {
  // The API refuses two nodes with one title for the same owner, so
  // same-titled results only arise from nodes shared by other users. Serve
  // that case from the lookup endpoint: it's exactly what the picker sees.
  const stamp = Date.now();
  const source = await createNode(request, `e2e dup source ${stamp}`);
  const title = `e2e dup ${stamp}`;
  const fake = (id: string, node_type: string) => ({ id, title, node_type });
  await page.route(/\/api\/nodes\/lookup\?/, async (route) => {
    if (!new URL(route.request().url()).searchParams.get('q')?.includes(`${stamp}`)) {
      return route.continue();
    }
    await route.fulfill({
      json: [
        fake('00000000-0000-4000-8000-00000000000a', 'article'),
        fake('00000000-0000-4000-8000-00000000000b', 'project'),
      ],
    });
  });
  try {
    const { combo, listbox, chip, confirm } = await openAddEdge(page, source.id);
    await combo.fill(title);
    await expect(listbox.getByRole('option')).toHaveCount(2);
    // The type tells them apart.
    await expect(listbox.getByRole('option').filter({ hasText: 'Project' })).toHaveCount(1);
    await combo.press('Enter');
    await expect(chip).toHaveCount(0);
    await expect(confirm).toBeDisabled();
    // An explicit choice commits.
    await combo.press('ArrowDown');
    await combo.press('Enter');
    await expect(chip).toContainText(title);
    await expect(confirm).toBeEnabled();
  } finally {
    await request.delete(`/api/nodes/${source.id}`);
  }
});

test('Escape closes only the list, then clears the text; blur closes the list', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const source = await createNode(request, `e2e esc source ${stamp}`);
  const target = await createNode(request, `e2e esc target ${stamp}`);
  try {
    const { combo, listbox, confirm } = await openAddEdge(page, source.id);
    await combo.fill(`e2e esc target ${stamp}`);
    await expect(listbox).toBeVisible();
    await combo.press('Escape');
    await expect(listbox).toBeHidden();
    await expect(combo).toHaveValue(`e2e esc target ${stamp}`);
    await combo.press('Escape');
    await expect(combo).toHaveValue('');
    // The form, and the page, are untouched.
    await expect(confirm).toBeVisible();
    await expect(page).toHaveURL(new RegExp(`/nodes/${source.id}$`));

    await combo.fill(`e2e esc target ${stamp}`);
    await expect(listbox).toBeVisible();
    // (The open list covers the Label field below it; the edge-type
    // select sits beside the picker.)
    await page.locator('select:has(option[value="references"])').click();
    await expect(listbox).toBeHidden();
  } finally {
    await request.delete(`/api/nodes/${source.id}`);
    await request.delete(`/api/nodes/${target.id}`);
  }
});

test('the Inbox add-task picker attaches by the same rules', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e attach node ${stamp}`);
  const title = `e2e attach task ${stamp}`;
  try {
    await page.goto('/tasks/inbox');
    const form = page.getByTestId('new-task-form');
    await form.getByPlaceholder('Task title…').fill(title);
    const combo = form.getByRole('combobox', { name: 'Link to a node (optional)…' });
    await combo.fill(`e2e attach node ${stamp}`);
    await combo.press('Enter');
    await expect(form.getByTestId('node-picker-chip')).toContainText(`e2e attach node ${stamp}`);
    await form.getByPlaceholder('Task title…').press('Enter');
    await expect
      .poll(async () => {
        const tasks: { title: string }[] = await (
          await request.get(`/api/nodes/${node.id}/tasks`)
        ).json();
        return tasks.some((t) => t.title === title);
      })
      .toBe(true);
  } finally {
    const tasks: { id: string }[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
    for (const t of tasks) await request.delete(`/api/tasks/${t.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

// ── Every node-choosing field is NodePicker (#130) ───────────────────────────

test('Add to Favorites picks a node with the shared picker', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e fav node ${stamp}`);
  try {
    await page.goto('/tasks/my-day');
    await page.locator('button[title="Add favorite"]').click();
    const dialog = page.getByRole('dialog', { name: 'Add to favorites' });
    const combo = dialog.getByRole('combobox', { name: 'Search nodes…' });
    await expect(combo).toBeFocused();
    const add = dialog.getByRole('button', { name: 'Add', exact: true });
    await expect(add).toBeDisabled();
    await combo.pressSequentially(`fav node ${stamp}`, { delay: 20 });
    await expect(dialog.getByRole('option')).toHaveCount(1);
    // Escape with the list open closes only the list, not the modal.
    await combo.press('Escape');
    await expect(dialog.getByRole('listbox')).toBeHidden();
    await expect(dialog).toBeVisible();
    await combo.press('ArrowDown');
    await combo.press('Enter');
    await expect(dialog.getByTestId('node-picker-chip')).toContainText(`e2e fav node ${stamp}`);
    await add.click();
    await expect
      .poll(async () => {
        const favs: { node_id: string | null }[] = await (await request.get('/api/favorites')).json();
        return favs.some((f) => f.node_id === node.id);
      })
      .toBe(true);
  } finally {
    const favs: { id: string; node_id: string | null }[] = await (
      await request.get('/api/favorites')
    ).json();
    for (const f of favs.filter((f) => f.node_id === node.id)) {
      await request.delete(`/api/favorites/${f.id}`);
    }
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('Inbox "Assign to node" commits with the shared picker', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e assign node ${stamp}`);
  const created = await request.post('/api/tasks', { data: { title: `e2e assign task ${stamp}` } });
  expect(created.ok()).toBeTruthy();
  const task = await created.json();
  try {
    await page.goto('/tasks/inbox');
    const row = page.locator(`[data-task-id="${task.id}"]`);
    await row.locator('button[title="Assign to node"]').click();
    const combo = row.getByRole('combobox', { name: 'Search nodes…' });
    await expect(combo).toBeFocused();
    await combo.fill(`e2e assign node ${stamp}`);
    await combo.press('Enter');
    await expect
      .poll(async () => {
        const tasks: { id: string }[] = await (await request.get(`/api/nodes/${node.id}/tasks`)).json();
        return tasks.some((t) => t.id === task.id);
      })
      .toBe(true);
  } finally {
    await request.delete(`/api/tasks/${task.id}`);
    await request.delete(`/api/nodes/${node.id}`);
  }
});
