import {
  test,
  expect,
  createNode,
  createNote,
  deleteNodesByPrefix,
  shareNode,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * Mentions of people in text (S9, #360; ONTOLOGY §7.3 and §14). Typing `@`
 * in a node's body or in a note lists the author's own People; a pick puts
 * `@[name](person:id)` into the text. The page shows the person's current
 * name: a link for their owner, plain text for anyone else, and the name as
 * typed, unresolved, when nobody may be named.
 *
 * Two identities: `request` (the page's default) and `otherRequest`. Every
 * person here has `MARK` in the name and every node starts with `NODE`;
 * `wipe` removes them for both identities before and after, so a crashed
 * attempt leaves nothing for its retry.
 */

const MARK = 'mention9x';
const NODE = 'e2e mention';
const OTHER = 'e2e-other-user';
const newStamp = () => `${MARK}${Date.now()}`;

type Person = { id: string; display_name: string };
type Name = { person_id: string; display_name: string; mine: boolean };

const token = (p: Person, label = p.display_name) => `@[${label}](person:${p.id})`;

async function wipe(...identities: APIRequestContext[]) {
  for (const who of identities) {
    const people: Person[] = await (await who.get(`/api/people?q=${MARK}`)).json();
    for (const p of people) await who.delete(`/api/people/${p.id}`);
    await deleteNodesByPrefix(who, NODE);
  }
}

async function person(who: APIRequestContext, data: Record<string, unknown>): Promise<Person> {
  const made = await who.post('/api/people', { data });
  expect(made.status(), await made.text()).toBe(201);
  return made.json();
}

async function saveBody(who: APIRequestContext, nodeId: string, body: string) {
  const saved = await who.put(`/api/nodes/${nodeId}`, { data: { body } });
  expect(saved.status(), await saved.text()).toBe(200);
}

/** The people a node's body mentions, as `who` is told. */
async function named(who: APIRequestContext, nodeId: string): Promise<Name[]> {
  const got = await who.get(`/api/nodes/${nodeId}`);
  expect(got.status()).toBe(200);
  return (await got.json()).mentions;
}

async function openEditor(page: Page, nodeId: string) {
  await page.goto(`/nodes/${nodeId}/edit`);
  const editor = page.getByPlaceholder(/Write in Markdown/);
  await expect(editor).toBeVisible();
  await editor.click();
  return editor;
}

const list = (page: Page) => page.getByTestId('mention-picker');

test('typing @ lists your people, and a pick by keyboard puts a mention in the text', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    emails: [{ value: `ada.${stamp}@example.com`, preferred: true }],
  });
  await person(request, { display_name: `e2e Bea ${stamp}` });
  const node = await createNode(request, `${NODE} minutes ${stamp}`, { body: '' });
  try {
    const editor = await openEditor(page, node.id);
    await editor.pressSequentially(`Met @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    // Name and email, and only the one who matches; the first is ready.
    const options = list(page).getByRole('option');
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText(ada.display_name);
    await expect(options.first()).toContainText(`ada.${stamp}@example.com`);
    await expect(options.first()).toHaveAttribute('aria-selected', 'true');
    await expect(editor).toHaveAttribute('aria-activedescendant', /mention-picker-\d+-opt-0/);

    await page.keyboard.press('Enter');
    await expect(list(page)).toHaveCount(0);
    await expect(editor).toHaveValue(`Met ${token(ada)} `);
    await expect(editor).toBeFocused();
    // The preview names them at once, as a link.
    await expect(page.locator('a.mention')).toHaveText(`@${ada.display_name}`);

    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    expect(await named(request, node.id)).toEqual([
      { person_id: ada.id, display_name: ada.display_name, mine: true },
    ]);
    // On the page: their name, a link to their card.
    const mention = page.locator('a.mention');
    await expect(mention).toHaveText(`@${ada.display_name}`);
    await mention.click();
    await expect(page).toHaveURL(new RegExp(`/people/${ada.id}$`));
    // And their card says where they are mentioned.
    const row = page.getByTestId('mentioned-in').getByTestId('mentioned-in-row');
    await expect(row).toHaveCount(1);
    await expect(row).toContainText(`${NODE} minutes ${stamp}`);
    await expect(row).toContainText('body');
    await row.getByRole('button').click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
  } finally {
    await wipe(request);
  }
});

test('@ then Enter is a new line; an email address opens nothing; Escape closes only the list', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const node = await createNode(request, `${NODE} keys ${stamp}`, { body: '' });
  try {
    const editor = await openEditor(page, node.id);
    // A bare @: the list shows, with nobody ready, so Enter is a new line.
    // A mention made by accident in shared text would tell a name.
    await editor.pressSequentially('@', { delay: 15 });
    await expect(list(page)).toBeVisible();
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(0);
    await page.keyboard.press('Enter');
    await expect(editor).toHaveValue('@\n');
    await expect(list(page)).toHaveCount(0);

    // An email address typed in text is not a mention being typed.
    await editor.pressSequentially(`write to ada@exam`, { delay: 15 });
    await expect(list(page)).toHaveCount(0);

    // Escape closes the list and nothing else: still editing, and the
    // list stays shut for that @.
    await editor.pressSequentially(` @Ada ${stamp}`.slice(0, -6), { delay: 15 });
    await expect(list(page)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(list(page)).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}/edit$`));
    await expect(editor).toBeFocused();
    await editor.pressSequentially('x', { delay: 15 });
    await expect(list(page)).toHaveCount(0);

    // From a bare @, an arrow key chooses, and then Enter picks.
    await editor.pressSequentially(' and @', { delay: 15 });
    await expect(list(page)).toBeVisible();
    await page.keyboard.press('ArrowDown');
    await expect(list(page).getByRole('option').first()).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Enter');
    expect(await editor.inputValue()).toMatch(/ and @\[e2e [A-Za-z]+ mention9x\d+\]\(person:[0-9a-f-]{36}\) $/);
    // A pointer picks too, and the text box keeps the keyboard.
    await editor.pressSequentially(`@Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await list(page).getByRole('option').first().click();
    await expect(editor).toBeFocused();
    expect(await editor.inputValue()).toContain(`${token(ada)} `);
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
  } finally {
    await wipe(request);
  }
});

test('a note mentions a person; Escape in the list keeps the edit; the trash leaves Mentioned in', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const node = await createNode(request, `${NODE} notes ${stamp}`, { body: 'no mention here' });
  const mentionedIn = async () => (await request.get(`/api/people/${ada.id}/mentions`)).json();
  try {
    await page.goto(`/nodes/${node.id}`);
    await page.locator('button[title="Add note"]').click();
    const box = page.getByPlaceholder('Write a note… (Ctrl+Enter to save)');
    await box.click();
    await box.pressSequentially(`Call @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await page.keyboard.press('Enter');
    await expect(box).toHaveValue(`Call ${token(ada)} `);
    await page.locator('button[title="Save note"]').click();

    const shown = page.locator('[data-note-id]').filter({ hasText: 'Call' });
    await expect(shown.locator('a.mention')).toHaveText(`@${ada.display_name}`);
    // By its id from here on: while it is edited the card holds its
    // words in a text box, where a text filter does not look.
    const noteId = await shown.getAttribute('data-note-id');
    const card = page.locator(`[data-note-id="${noteId}"]`);
    expect(await mentionedIn()).toEqual([
      expect.objectContaining({ node_id: node.id, in_body: false, notes: 1 }),
    ]);

    // Editing it: Escape with the list open closes the list, not the edit.
    await card.locator('button[title="Edit note"]').click();
    const edit = card.getByPlaceholder('Note…');
    await edit.click();
    await edit.press('End');
    await edit.pressSequentially('and @', { delay: 15 });
    await expect(list(page)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(list(page)).toHaveCount(0);
    await expect(edit).toBeVisible();
    expect(await edit.inputValue()).toContain('and @');
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    // Saved with what was typed: the mention is still named on the card.
    await card.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(card.locator('a.mention')).toHaveText(`@${ada.display_name}`);

    // In the trash it is in nobody's "Mentioned in".
    expect((await request.delete(`/api/notes/${noteId}`)).status()).toBe(204);
    expect(await mentionedIn()).toEqual([]);
  } finally {
    await wipe(request);
  }
});

test('someone the node is shared with sees the name and nothing else', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = newStamp();
  await wipe(request, otherRequest);
  const ada = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    company: `Secret Co ${stamp}`,
    emails: [{ value: `ada.${stamp}@example.com` }],
    phones: [{ value: '+1 555 010 4242' }],
    notes: `private ${stamp}`,
  });
  const node = await createNode(request, `${NODE} shared ${stamp}`, { body: `Met ${token(ada)}.` });
  await createNote(request, node.id, `Note about ${token(ada)}.`);
  await shareNode(request, node.id, OTHER, 'editor');
  try {
    // What the API tells them: the id, the name and "not yours".
    const theirs = await otherRequest.get(`/api/nodes/${node.id}`);
    const body = await theirs.json();
    expect(body.mentions).toEqual([{ person_id: ada.id, display_name: ada.display_name, mine: false }]);
    const notes = await otherRequest.get(`/api/nodes/${node.id}/notes`);
    expect((await notes.json())[0].mentions).toEqual([
      { person_id: ada.id, display_name: ada.display_name, mine: false },
    ]);
    const feed = await otherRequest.get('/api/notes/feed');
    for (const answer of [JSON.stringify(body), await notes.text(), await feed.text()]) {
      for (const secret of ['example.com', '4242', 'Secret Co', 'private mention9x']) {
        expect(answer, secret).not.toContain(secret);
      }
    }
    // No way into the registry: the person and their list read as missing.
    expect((await otherRequest.get(`/api/people/${ada.id}`)).status()).toBe(404);
    expect((await otherRequest.get(`/api/people/${ada.id}/mentions`)).status()).toBe(404);

    // On their page: the name as plain text, in the body and in the note.
    await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('span.mention')).toHaveCount(2);
    await expect(page.locator('span.mention').first()).toHaveText(`@${ada.display_name}`);
    await expect(page.locator('a.mention')).toHaveCount(0);
    await expect(page.locator('main')).not.toContainText('example.com');

    // A rename shows to them, while the person's owner can open the node.
    const renamed = await request.put(`/api/people/${ada.id}`, {
      data: { display_name: `e2e Ada King ${stamp}`, emails: [{ value: `ada.${stamp}@example.com` }] },
    });
    expect(renamed.status()).toBe(200);
    await page.reload();
    await expect(page.locator('span.mention').first()).toHaveText(`@e2e Ada King ${stamp}`);
  } finally {
    await page.setExtraHTTPHeaders({});
    await wipe(request, otherRequest);
  }
});

test('a token with someone else’s id names nobody, whoever saves it', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = newStamp();
  await wipe(request, otherRequest);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const node = await createNode(request, `${NODE} source ${stamp}`, { body: `Met ${token(ada)}.` });
  await shareNode(request, node.id, OTHER, 'editor');
  try {
    // Typed into a node of their own: no name, for them or anyone.
    const theirs = await createNode(otherRequest, `${NODE} forged ${stamp}`, {
      body: `I know ${token(ada, 'whoever this is')}.`,
    });
    expect(await named(otherRequest, theirs.id)).toEqual([]);
    // A copy of the shared node is theirs, and its token is not.
    const copy = await otherRequest.post(`/api/nodes/${node.id}/duplicate`);
    expect(copy.ok(), await copy.text()).toBeTruthy();
    expect(await named(otherRequest, (await copy.json()).id)).toEqual([]);

    // Cut out of the shared body and put back by them: plain text until
    // the person's owner saves. It can understate; it never reveals.
    await saveBody(otherRequest, node.id, 'taken out');
    await saveBody(otherRequest, node.id, `Met ${token(ada)}.`);
    expect(await named(request, node.id)).toEqual([]);
    expect(await named(otherRequest, node.id)).toEqual([]);
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('.mention-unresolved')).toHaveText(`@${ada.display_name}`);
    await expect(page.locator('a.mention')).toHaveCount(0);
    await saveBody(request, node.id, `Met ${token(ada)}.`);
    expect(await named(request, node.id)).toEqual([
      { person_id: ada.id, display_name: ada.display_name, mine: true },
    ]);

    // As the other identity, the forged token reads as typed, unresolved.
    await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
    await page.goto(`/nodes/${theirs.id}`);
    await expect(page.locator('.mention-unresolved')).toHaveText('@whoever this is');
    await expect(page.locator('.mention')).toHaveCount(0);
  } finally {
    await page.setExtraHTTPHeaders({});
    await wipe(request, otherRequest);
  }
});

test('when the one who mentioned loses access, their person is no longer named', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = newStamp();
  await wipe(request, otherRequest);
  const bo = await person(otherRequest, { display_name: `e2e Bo ${stamp}` });
  const node = await createNode(request, `${NODE} mine ${stamp}`, { body: 'nothing yet' });
  const grant = await request.post(`/api/nodes/${node.id}/permissions`, {
    data: { subject_id: OTHER, role: 'editor' },
  });
  expect(grant.ok()).toBeTruthy();
  try {
    await saveBody(otherRequest, node.id, `Ask ${token(bo)}.`);
    // The node's owner is told the name, and that the person is not theirs.
    expect(await named(request, node.id)).toEqual([
      { person_id: bo.id, display_name: bo.display_name, mine: false },
    ]);
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('span.mention')).toHaveText(`@${bo.display_name}`);

    // They stop sharing. The text is the same; the name is not given any
    // more, or the owner would go on reading it through every rename.
    const revoked = await request.delete(`/api/nodes/${node.id}/permissions/${(await grant.json()).id}`);
    expect(revoked.ok(), await revoked.text()).toBeTruthy();
    await otherRequest.put(`/api/people/${bo.id}`, { data: { display_name: `e2e Bo Renamed ${stamp}` } });
    expect(await named(request, node.id)).toEqual([]);
    await page.reload();
    await expect(page.locator('.mention-unresolved')).toHaveText(`@${bo.display_name}`);
    await expect(page.locator('main')).not.toContainText('Bo Renamed');
  } finally {
    await wipe(request, otherRequest);
  }
});

test('a deleted person’s mention keeps the name as typed, and the delete says so', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const node = await createNode(request, `${NODE} after ${stamp}`, { body: `Met ${token(ada)}.` });
  try {
    await page.goto(`/people/${ada.id}`);
    const asked: string[] = [];
    page.once('dialog', (dialog) => {
      asked.push(dialog.message());
      void dialog.accept();
    });
    await page.getByRole('button', { name: 'Delete person' }).click();
    await expect(page).toHaveURL(/\/people$/);
    expect(asked.join(' ')).toContain('the name as typed stays');

    expect(await named(request, node.id)).toEqual([]);
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('.mention-unresolved')).toHaveText(`@${ada.display_name}`);
    await expect(page.locator('a.mention')).toHaveCount(0);
  } finally {
    await wipe(request);
  }
});

test('after a rename the editor offers to update the name in the text, and never does it unasked', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const before = `Met ${token(ada)}.`;
  const node = await createNode(request, `${NODE} rename ${stamp}`, { body: before });
  const renamed = { ...ada, display_name: `e2e Ada King ${stamp}` };
  expect(
    (await request.put(`/api/people/${ada.id}`, { data: { display_name: renamed.display_name } })).status(),
  ).toBe(200);
  try {
    // The page shows the current name at once; the text is as it was.
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('a.mention')).toHaveText(`@${renamed.display_name}`);
    expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe(before);

    const editor = await openEditor(page, node.id);
    const notice = page.getByTestId('stale-mentions');
    await expect(notice).toContainText('1 mention shows an older name.');
    await expect(editor).toHaveValue(before);
    // Leaving without pressing it changes nothing.
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe(before);

    await openEditor(page, node.id);
    await page.getByTestId('stale-mentions').getByRole('button', { name: 'Update' }).click();
    await expect(page.getByPlaceholder(/Write in Markdown/)).toHaveValue(`Met ${token(renamed)}.`);
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe(`Met ${token(renamed)}.`);
  } finally {
    await wipe(request);
  }
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the list of people fits', async ({ page, request }) => {
    const stamp = newStamp();
    await wipe(request);
    await person(request, {
      display_name: `e2e Ada With A Rather Long Name ${stamp}`,
      emails: [{ value: `ada.with.a.rather.long.address.${stamp}@example.com` }],
    });
    const node = await createNode(request, `${NODE} phone ${stamp}`, { body: '' });
    try {
      const editor = await openEditor(page, node.id);
      await editor.pressSequentially('@Ada', { delay: 15 });
      await expect(list(page)).toBeVisible();
      const box = await list(page).boundingBox();
      expect(box && box.x).toBeGreaterThanOrEqual(0);
      expect(box && box.x + box.width).toBeLessThanOrEqual(375);
      expect(await list(page).evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(1);
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth),
      ).toBeLessThanOrEqual(1);
      await page.keyboard.press('Escape');
    } finally {
      await wipe(request);
    }
  });
});
