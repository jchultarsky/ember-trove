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
    // The registry is read when the first @ is typed, once: not with the
    // editor, and not again at every keystroke after it.
    const registryReads: string[] = [];
    page.on('request', (r) => {
      const url = new URL(r.url());
      if (r.method() === 'GET' && url.pathname === '/api/people') registryReads.push(url.search);
    });
    const editor = await openEditor(page, node.id);
    expect(registryReads).toHaveLength(0);
    await editor.pressSequentially(`Met @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    // Name and email, and only the one who matches; the first is ready.
    const options = list(page).getByRole('option');
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText(ada.display_name);
    await expect(options.first()).toContainText(`ada.${stamp}@example.com`);
    await expect(options.first()).toHaveAttribute('aria-selected', 'true');
    // Tied to the text box for a screen reader, by the person, and said.
    await expect(editor).toHaveAttribute(
      'aria-activedescendant',
      new RegExp(`mention-picker-\\d+-${ada.id.replace(/-/g, '')}`),
    );
    await expect(page.getByTestId('mention-heard')).toHaveText(`1 person. ${ada.display_name} highlighted.`);

    await page.keyboard.press('Enter');
    await expect(list(page)).toHaveCount(0);
    await expect(editor).toHaveValue(`Met ${token(ada)} `);
    await expect(editor).toBeFocused();
    // The preview names them at once, as a link.
    await expect(page.locator('a.mention')).toHaveText(`@${ada.display_name}`);
    expect(registryReads).toHaveLength(1);
    // The pick is one edit of the box: undo takes it back, redo puts it in.
    await page.keyboard.press('ControlOrMeta+z');
    expect(await editor.inputValue()).not.toContain('person:');
    await page.keyboard.press('ControlOrMeta+Shift+z');
    await expect(editor).toHaveValue(`Met ${token(ada)} `);
    // A new @ reads the registry again, so someone added since is
    // offered; typing on after it does not.
    const cy = await person(request, { display_name: `e2e Cy ${stamp}` });
    await editor.press('End');
    await editor.pressSequentially(`@Cy ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText(cy.display_name);
    expect(registryReads).toHaveLength(2);
    await page.keyboard.press('Escape');
    for (const _ of `@Cy ${stamp}`.slice(0, -4)) await page.keyboard.press('Backspace');
    await expect(editor).toHaveValue(`Met ${token(ada)} `);

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
  const ada = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    company: `Secret Co ${stamp}`,
    emails: [{ value: `zed.${stamp}@example.com` }],
    phones: [{ value: '+1 555 010 4242' }],
  });
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
    expect(await editor.inputValue()).toMatch(/ and @\[[^\]]+\]\(person:[0-9a-f-]{36}\) $/);
    // A pointer picks too, and the text box keeps the keyboard.
    await editor.pressSequentially(`@Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await list(page).getByRole('option').first().click();
    await expect(editor).toBeFocused();
    expect(await editor.inputValue()).toContain(`${token(ada)} `);

    // Who is offered: by a word of the name or the start of an address.
    // Never by a phone number or a company: "lunch @12" then Enter must
    // not put whoever has 12 in a number into a text others read.
    await editor.pressSequentially('lunch @4242', { delay: 15 });
    await expect(list(page)).toHaveCount(0);
    await editor.pressSequentially(' see @Secret', { delay: 15 });
    await expect(list(page)).toHaveCount(0);
    // By the start of an address they are listed, with nobody ready:
    // Enter is a new line until an arrow key chooses.
    await editor.pressSequentially(` @zed.${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(0);
    await expect(page.getByTestId('mention-heard')).toHaveText('1 person. Arrow down to choose.');
    // Shift+Tab leaves the field and picks nobody.
    await page.keyboard.press('Shift+Tab');
    await expect(list(page)).toHaveCount(0);
    expect(await editor.inputValue()).toMatch(/@zed\.mention9x\d+$/);
    // The same with someone highlighted: Shift+Tab never picks.
    await editor.focus();
    await editor.press('End');
    await editor.pressSequentially(` @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    await page.keyboard.press('Shift+Tab');
    await expect(list(page)).toHaveCount(0);
    expect(await editor.inputValue()).toMatch(/ @Ada mention9x\d+$/);
    // The list follows the cursor when a key moves it and types nothing.
    // Moved into an @… that is in the text already, nobody is ready: Enter
    // at the end of a line that ends in "@bob" is a new line, as anywhere.
    await editor.focus();
    await editor.press('End');
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(0);
    await page.keyboard.press('Enter');
    await expect(list(page)).toHaveCount(0);
    expect(await editor.inputValue()).toMatch(/ @Ada mention9x\d+\n$/);
    // Home leaves the mention being typed, End comes back to it. (On a
    // short line of its own: Home goes to the start of the line as it is
    // laid out, which on a long wrapped line is not the start.)
    await editor.pressSequentially(`@Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    await editor.press('Home');
    await expect(list(page)).toHaveCount(0);
    await editor.press('End');
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(0);
    // A click into it is the same; typing on makes the match ready again.
    await editor.press('Home');
    await expect(list(page)).toHaveCount(0);
    await editor.evaluate((el: HTMLTextAreaElement) => {
      el.setSelectionRange(el.value.length, el.value.length);
      el.click();
    });
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(0);
    await editor.pressSequentially(stamp.slice(-4, -3), { delay: 15 });
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    // A key with a modifier that is not the save chord leaves the list.
    await page.keyboard.press('Control');
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    // An arrow key the list used keeps its highlight when the key comes up.
    await person(request, { display_name: `e2e Ada Two ${stamp}` });
    await editor.pressSequentially(` x @${stamp}`, { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(2);
    await page.keyboard.press('ArrowDown');
    await expect(list(page).getByRole('option').nth(1)).toHaveAttribute('aria-selected', 'true');
    // Closed with Escape it stays closed for that @, until the cursor has
    // been somewhere else.
    await page.keyboard.press('Escape');
    await editor.pressSequentially('x', { delay: 15 });
    await page.keyboard.press('Backspace');
    await expect(list(page)).toHaveCount(0);
    await editor.press('Home');
    await editor.press('End');
    await expect(list(page).getByRole('option')).toHaveCount(2);
    await page.keyboard.press('Escape');
    // Tab picks a highlighted one, after text that is not ASCII (the
    // cursor is counted in UTF-16 units, the text in bytes).
    await editor.focus();
    await editor.press('End');
    await editor.pressSequentially(` 👉 — @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    await page.keyboard.press('Tab');
    expect(await editor.inputValue()).toMatch(new RegExp(` 👉 — @\\[e2e Ada ${stamp}\\]\\(person:${ada.id}\\) $`));
    await expect(editor).toBeFocused();
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
    await expect(list(page)).toBeInViewport({ ratio: 1 });
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

    // A note written without a mention, then edited to hold one: the
    // card is drawn from the save's own answer, names included.
    await page.locator('button[title="Add note"]').click();
    await box.fill(`plain words ${stamp}`);
    await page.locator('button[title="Save note"]').click();
    const plain = page.locator('[data-note-id]').filter({ hasText: `plain words ${stamp}` });
    await expect(plain).toBeVisible();
    const plainId = await plain.getAttribute('data-note-id');
    const second = page.locator(`[data-note-id="${plainId}"]`);
    await second.locator('button[title="Edit note"]').click();
    const secondEdit = second.getByPlaceholder('Note…');
    await secondEdit.press('End');
    await secondEdit.pressSequentially(` with @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await page.keyboard.press('Enter');
    await second.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(second.locator('a.mention')).toHaveText(`@${ada.display_name}`);

    // Ctrl/⌘+Enter with the list open, someone highlighted, saves what was
    // typed: it picks nobody, and the list goes.
    await page.locator('button[title="Add note"]').click();
    await box.pressSequentially(`typed @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    await page.keyboard.press('ControlOrMeta+Enter');
    await expect(list(page)).toHaveCount(0);
    const typed = page.locator('[data-note-id]').filter({ hasText: 'typed @Ada' });
    await expect(typed).toBeVisible();
    await expect(typed.locator('.mention, .mention-unresolved')).toHaveCount(0);

    // After a rename the card shows the new name, and editing the note
    // changes nothing in its text unasked: the editor says the name is
    // older and offers to update it.
    const king = `e2e Ada King ${stamp}`;
    expect((await request.put(`/api/people/${ada.id}`, { data: { display_name: king } })).status()).toBe(200);
    await page.reload();
    await expect(card.locator('a.mention')).toHaveText(`@${king}`);
    await card.locator('button[title="Edit note"]').click();
    const again = card.getByPlaceholder('Note…');
    await expect(card.getByTestId('stale-mentions')).toContainText('1 mention shows an older name.');
    await again.press('End');
    await again.pressSequentially(' more words', { delay: 5 });
    await card.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(card.getByText('more words')).toBeVisible();
    const saved: { id: string; body: string }[] = await (await request.get(`/api/nodes/${node.id}/notes`)).json();
    expect(saved.find((n) => n.id === noteId)?.body).toContain(token(ada));
    // "Update" does it, as an edit the author makes.
    await card.locator('button[title="Edit note"]').click();
    await card.getByTestId('stale-mentions').getByRole('button', { name: 'Update' }).click();
    await expect(card.getByTestId('stale-mentions')).toHaveCount(0);
    await card.getByRole('button', { name: 'Save', exact: true }).click();
    await expect
      .poll(async () => {
        const notes: { id: string; body: string }[] = await (await request.get(`/api/nodes/${node.id}/notes`)).json();
        return notes.find((n) => n.id === noteId)?.body ?? '';
      })
      .toContain(token({ ...ada, display_name: king }));

    // In the trash a note is in nobody's "Mentioned in".
    expect(await mentionedIn()).toEqual([expect.objectContaining({ node_id: node.id, notes: 2 })]);
    expect((await request.delete(`/api/notes/${noteId}`)).status()).toBe(204);
    expect(await mentionedIn()).toEqual([expect.objectContaining({ node_id: node.id, notes: 1 })]);
    expect((await request.delete(`/api/notes/${plainId}`)).status()).toBe(204);
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

    // In their editor (they may edit): the preview names the person the
    // text came with, as plain text, and offers no update of a name that
    // is not theirs.
    await page.goto(`/nodes/${node.id}/edit`);
    await expect(page.getByPlaceholder(/Write in Markdown/)).toBeVisible();
    await expect(page.locator('span.mention')).toHaveText(`@${ada.display_name}`);
    await expect(page.locator('a.mention')).toHaveCount(0);
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('span.mention')).toHaveCount(2);

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
    // the person's owner mentions them again. It can understate; it
    // never reveals.
    await saveBody(otherRequest, node.id, 'taken out');
    await saveBody(otherRequest, node.id, `Met ${token(ada)}.`);
    expect(await named(request, node.id)).toEqual([]);
    expect(await named(otherRequest, node.id)).toEqual([]);
    await page.goto(`/nodes/${node.id}`);
    await expect(page.locator('.mention-unresolved')).toHaveText(`@${ada.display_name}`);
    await expect(page.locator('a.mention')).toHaveCount(0);
    // Her own save around it does not make it real either: she did not
    // put it there. (A token planted with her person's id, hidden in a
    // comment, would otherwise be named to the planter by her next save.)
    await saveBody(request, node.id, `Met ${token(ada)}. And a word of hers.`);
    expect(await named(request, node.id)).toEqual([]);
    // In her editor the token is not shown as a mention, and its name
    // is not offered for update: "Update" would write her person's
    // current name into text the other one reads.
    expect((await request.put(`/api/people/${ada.id}`, { data: { display_name: `e2e Ada King ${stamp}` } })).status()).toBe(200);
    await page.goto(`/nodes/${node.id}/edit`);
    await expect(page.getByPlaceholder(/Write in Markdown/)).toBeVisible();
    await expect(page.locator('.mention-unresolved')).toHaveText(`@${ada.display_name}`);
    await expect(page.locator('a.mention')).toHaveCount(0);
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    expect((await request.put(`/api/people/${ada.id}`, { data: { display_name: ada.display_name } })).status()).toBe(200);
    // She mentions them again herself: out, then in, in saves of her own.
    await saveBody(request, node.id, 'taken out by its owner');
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

    // "Keep as typed" puts the notice away and changes nothing.
    await openEditor(page, node.id);
    await page.getByTestId('stale-mentions').getByRole('button', { name: 'Keep as typed' }).click();
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    await expect(page.getByPlaceholder(/Write in Markdown/)).toHaveValue(before);
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));

    // "Keep as typed" is for the names it was pressed for. Another
    // person's older name is still offered afterwards, and "Update"
    // then leaves the kept one alone.
    const bea = await person(request, { display_name: `e2e Bea ${stamp}` });
    const kept = await openEditor(page, node.id);
    await page.getByTestId('stale-mentions').getByRole('button', { name: 'Keep as typed' }).click();
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    await kept.press('End');
    await kept.pressSequentially(` @Bea ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).locator('[aria-selected="true"]')).toHaveCount(1);
    await page.keyboard.press('Enter');
    await expect(kept).toHaveValue(`${before} ${token(bea)} `);
    await kept.fill(`${before} ${token(bea, 'B')}`);
    await expect(page.getByTestId('stale-mentions')).toContainText('1 mention shows an older name.');
    await page.getByTestId('stale-mentions').getByRole('button', { name: 'Update' }).click();
    await expect(kept).toHaveValue(`${before} ${token(bea)}`);
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    await kept.fill(before);
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe(before);

    const editor3 = await openEditor(page, node.id);
    // With the cursor inside the token (in its label):
    await editor3.evaluate((el: HTMLTextAreaElement) => el.setSelectionRange(10, 10));
    await page.getByTestId('stale-mentions').getByRole('button', { name: 'Update' }).click();
    await expect(editor3).toHaveValue(`Met ${token(renamed)}.`);
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    // The cursor is after the token, never inside it: the next key typed
    // must not break the mention.
    expect(await editor3.evaluate((el: HTMLTextAreaElement) => [el.selectionStart, el.selectionEnd])).toEqual([
      `Met ${token(renamed)}`.length,
      `Met ${token(renamed)}`.length,
    ]);
    // It is one edit of the box, with the keyboard back in it: undo takes
    // it back (and the notice with it), redo makes it again.
    await expect(editor3).toBeFocused();
    await page.keyboard.press('ControlOrMeta+z');
    await expect(editor3).toHaveValue(before);
    await expect(page.getByTestId('stale-mentions')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+Shift+z');
    await expect(editor3).toHaveValue(`Met ${token(renamed)}.`);
    await expect(page.getByTestId('stale-mentions')).toHaveCount(0);
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
    expect((await (await request.get(`/api/nodes/${node.id}`)).json()).body).toBe(`Met ${token(renamed)}.`);

    // A shorter name: the edit only takes characters out, and is still
    // one edit. The cursor, at the end of the text, stays at the end.
    const short = { ...ada, display_name: `e2e Ad ${stamp}` };
    expect((await request.put(`/api/people/${ada.id}`, { data: { display_name: short.display_name } })).status()).toBe(
      200,
    );
    const editor4 = await openEditor(page, node.id);
    await editor4.evaluate((el: HTMLTextAreaElement) => el.setSelectionRange(el.value.length, el.value.length));
    await page.getByTestId('stale-mentions').getByRole('button', { name: 'Update' }).click();
    await expect(editor4).toHaveValue(`Met ${token(short)}.`);
    expect(await editor4.evaluate((el: HTMLTextAreaElement) => el.selectionStart)).toBe(`Met ${token(short)}.`.length);
    await page.keyboard.press('ControlOrMeta+z');
    await expect(editor4).toHaveValue(`Met ${token(renamed)}.`);
  } finally {
    await wipe(request);
  }
});

test('the Notes page: the list opens under the box, in view, and a pick is posted', async ({ page, request }) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  await person(request, { display_name: `e2e Adam ${stamp}` });
  await person(request, { display_name: `e2e Adele ${stamp}` });
  const node = await createNode(request, `${NODE} feed ${stamp}`, { body: 'x' });
  try {
    await page.goto('/notes');
    const box = page.getByPlaceholder('Write a note…  (Ctrl+Enter to post)');
    await box.click();
    // Several people (the stamp is a word of each name): the whole list
    // is in view, not cut off above the box. The first by name is ready.
    await box.pressSequentially(`Seen @${stamp}`, { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(3);
    await expect(list(page)).toBeInViewport({ ratio: 1 });
    await expect(list(page).getByRole('option').first()).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Enter');
    await expect(box).toHaveValue(`Seen ${token(ada)} `);

    const picker = page.getByRole('combobox', { name: 'Note on a node…' });
    await picker.fill(`${NODE} feed ${stamp}`);
    // The picker's own list: the page's sort and filter selects have
    // options too.
    await expect(page.locator('[role="listbox"] [role="option"][aria-selected="true"]')).toContainText(
      `${NODE} feed ${stamp}`,
    );
    await picker.press('Enter');
    await box.focus();
    await page.keyboard.press('ControlOrMeta+Enter');
    // Posted, and shown in the feed with their name as a link.
    await expect(page.locator('a.mention').first()).toHaveText(`@${ada.display_name}`);
    await expect(list(page)).toHaveCount(0);
    expect(await (await request.get(`/api/people/${ada.id}/mentions`)).json()).toEqual([
      expect.objectContaining({ node_id: node.id, in_body: false, notes: 1 }),
    ]);
  } finally {
    await wipe(request);
  }
});

test('the registry arriving late, failing, and coming back', async ({ page, request }) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const node = await createNode(request, `${NODE} registry ${stamp}`, { body: '' });
  const registry = (url: URL) => url.pathname === '/api/people';
  try {
    const editor = await openEditor(page, node.id);
    // The answer is held until the whole name is typed: when it comes,
    // the one who matches is ready for Enter, as if it had been there.
    let release: () => void = () => {};
    const held = new Promise<void>((done) => (release = done));
    await page.route(registry, async (route) => {
      await held;
      await route.continue();
    });
    await editor.pressSequentially(`Met @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page)).toHaveCount(0);
    release();
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).getByRole('option').first()).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Enter');
    await expect(editor).toHaveValue(`Met ${token(ada)} `);
    await page.unroute(registry);

    // A read that fails is said, not shown as "nobody".
    await page.route(registry, (route) => route.fulfill({ status: 500, body: 'no' }));
    await editor.pressSequentially('and @', { delay: 15 });
    const alert = list(page).getByRole('alert');
    await expect(alert).toContainText('Couldn’t load your People');
    // Escape puts it away and nothing else: still editing, text kept.
    await page.keyboard.press('Escape');
    await expect(list(page)).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}/edit$`));
    await expect(editor).toBeFocused();
    await expect(editor).toHaveValue(`Met ${token(ada)} and @`);
    // Typing @ again reads it again.
    await page.unroute(registry);
    await editor.pressSequentially(` @Ada ${stamp}`.slice(0, -4), { delay: 15 });
    await expect(list(page).getByRole('option')).toHaveCount(1);
    await expect(list(page).getByRole('alert')).toHaveCount(0);
    await page.keyboard.press('Enter');
    await expect(editor).toHaveValue(`Met ${token(ada)} and @ ${token(ada)} `);

    // The highlight is on a person, not on a row: when the list changes
    // under it, Enter still picks who was highlighted.
    await person(request, { display_name: `e2e Aaa ${stamp}` });
    let arrive: () => void = () => {};
    const waiting = new Promise<void>((done) => (arrive = done));
    await page.route(registry, async (route) => {
      await waiting;
      await route.continue();
    });
    await editor.pressSequentially(`@${stamp}`, { delay: 15 });
    const options = list(page).getByRole('option');
    await expect(options).toHaveCount(1);
    await page.keyboard.press('ArrowDown');
    await expect(options.first()).toHaveAttribute('aria-selected', 'true');
    arrive();
    await expect(options).toHaveCount(2);
    await expect(options.first()).toContainText(`e2e Aaa ${stamp}`);
    await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
    await expect(options.nth(1)).toContainText(ada.display_name);
    await page.keyboard.press('Enter');
    await expect(editor).toHaveValue(`Met ${token(ada)} and @ ${token(ada)} ${token(ada)} `);
    await page.unroute(registry);
    await editor.fill('');
    await page.getByRole('button', { name: 'Done', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${node.id}$`));
  } finally {
    await wipe(request);
  }
});

test('a mention is shown only where Markdown reads a link', async ({ page, request }) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada Secret ${stamp}` });
  // A real mention, and the same token in a link's address, an
  // attribute and code: a name in a writer's own address would travel
  // there on a click.
  const body = [
    `Met ${token(ada, 'A')}.`,
    `<a href='https://attacker.example/?q=${token(ada, 'A')}'>Quarterly report</a>`,
    `[a link](https://attacker.example/${token(ada, 'A')})`,
    `\`${token(ada, 'A')}\``,
    // In an image's alt text, and as the words of a link the writer
    // wrote by hand: not a mention there either.
    `![${token(ada, 'A')}](/favicon.ico)`,
    `<a href="https://attacker.example/">${token(ada, 'A')}</a>`,
    // What broke out of the sanitizer while a mention's markup was put
    // in after it: forged markers around an attribute.
    `<a href="https://attacker.example/" title="&#xE000;">Sign in again</a>&#xE001;<span title=" class=etl-root style=position:fixed;inset:0 onmouseover=alert(1) x=">.</span>`,
    // A writer may type the class. It is a word on their own link, and
    // does not get the look of a link to a person.
    `<a class="mention" href="https://attacker.example/">@Boss</a>`,
  ].join('\n\n');
  const node = await createNode(request, `${NODE} places ${stamp}`, { body });
  try {
    await page.goto(`/nodes/${node.id}`);
    const real = page.locator('a.mention[href^="/people/"]');
    await expect(real).toHaveCount(1);
    await expect(real).toHaveText(`@${ada.display_name}`);
    const typed = page.locator('a.mention:not([href^="/people/"])');
    await expect(typed).toHaveText('@Boss');
    const colour = (el: Element) => getComputedStyle(el).color;
    expect(await typed.evaluate(colour)).not.toBe(await real.evaluate(colour));
    const html = await page.locator('.prose').first().innerHTML();
    expect(html.match(/Secret/g) ?? []).toHaveLength(1);
    // Nothing a writer typed became a class, a handler or a position.
    expect(await page.locator('.prose [class~="etl-root"], .prose [onmouseover]').count()).toBe(0);
    for (const el of await page.locator('.prose [style]').all()) {
      expect(await el.getAttribute('style')).not.toMatch(/position|inset/);
    }
    for (const link of await page.locator('.prose a[href^="http"]').all()) {
      expect(await link.getAttribute('href')).not.toContain('Secret');
    }
  } finally {
    await wipe(request);
  }
});

test('a text made to swallow a mention into a link’s address names nobody', async ({ page, request }) => {
  const stamp = newStamp();
  await wipe(request);
  const ada = await person(request, { display_name: `e2e Ada Secret ${stamp}` });
  // Raw HTML that ends inside an open attribute: to the browser, what
  // Markdown writes next is that attribute's value. The name would be in
  // the writer's address. The page checks what it rendered, and shows
  // the whole text with nobody named.
  const body = [
    `Intro ${token(ada, 'A')}.`,
    `<div></a><a href='https://attacker.example/?q=`,
    `Thanks ${token(ada, 'A')} for the review.`,
    `' <b>Quarterly report</b></a></div>`,
    `Outro ${token(ada, 'A')}.`,
  ].join('\n\n');
  const node = await createNode(request, `${NODE} swallowed ${stamp}`, { body });
  try {
    // The API does name them: the page is what holds the name back.
    expect(await named(request, node.id)).toEqual([
      { person_id: ada.id, display_name: ada.display_name, mine: true },
    ]);
    await page.goto(`/nodes/${node.id}`);
    const prose = page.locator('.prose').first();
    await expect(prose).toContainText('Intro');
    await expect(prose.locator('.mention-unresolved').first()).toHaveText('@A');
    await expect(prose.locator('.mention')).toHaveCount(0);
    expect(await prose.innerHTML()).not.toContain('Secret');
    for (const link of await prose.locator('a[href]').all()) {
      expect(await link.getAttribute('href')).not.toContain('Secret');
    }
    // The same in the editor's preview.
    await openEditor(page, node.id);
    await expect(page.locator('.mention-unresolved').first()).toHaveText('@A');
    await expect(page.locator('a.mention[href^="/people/"]')).toHaveCount(0);
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

  test('a long name in the text wraps', async ({ page, request }) => {
    const stamp = newStamp();
    await wipe(request);
    const long = await person(request, {
      display_name: `e2e ${'Wolfeschlegelsteinhausenbergerdorff'.repeat(3)} ${stamp}`,
    });
    const node = await createNode(request, `${NODE} long ${stamp}`, {
      body: `Met ${token(long)} and ${token({ ...long, id: '00000000-0000-4000-8000-000000000000' }, 'Nobody'.repeat(30))}.`,
    });
    try {
      await page.goto(`/nodes/${node.id}`);
      await expect(page.locator('a.mention')).toBeVisible();
      await expect(page.locator('.mention-unresolved')).toBeVisible();
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth),
      ).toBeLessThanOrEqual(1);
      const box = await page.locator('a.mention').boundingBox();
      expect(box && box.x + box.width).toBeLessThanOrEqual(375);
    } finally {
      await wipe(request);
    }
  });
});
