import {
  test,
  expect,
  createNode,
  createProject,
  deleteNodesByPrefix,
  shareNode,
  type APIRequestContext,
  type Page,
} from './fixtures';

/**
 * Merging two saved People (S8c, #363; ONTOLOGY §7.1).
 *
 * One of two records of the same person is kept; the other's details,
 * memberships and mentions move to it, and the other is deleted. The dialog
 * shows the server's plan first. It opens from a person's page, from the
 * notice that refuses an address another saved person has, and from Data
 * health. No text is rewritten: an old mention shows the kept person's name.
 */

const MARK = 'merge8c';
const NODE = 'e2e merge';
const OTHER = 'e2e-other-user';
const newStamp = () => `${MARK}${Date.now()}`;

type Person = {
  id: string;
  display_name: string;
  position?: string | null;
  notes?: string | null;
  emails: { value: string }[];
  updated_at: string;
};

const token = (p: Person) => `@[${p.display_name}](person:${p.id})`;

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

async function putOn(who: APIRequestContext, nodeId: string, p: Person, role?: string, note?: string) {
  const added = await who.post(`/api/nodes/${nodeId}/people`, { data: { person_id: p.id, role, note } });
  expect(added.ok(), await added.text()).toBeTruthy();
}

/** Merge `gone` into `kept` through the API, with the plan as it is now. */
async function mergeByApi(who: APIRequestContext, kept: Person, gone: Person) {
  const plan = await who.get(`/api/people/${kept.id}/merge?from=${gone.id}`);
  expect(plan.status(), await plan.text()).toBe(200);
  const merged = await who.post(`/api/people/${kept.id}/merge`, {
    data: { from: gone.id, confirm: (await plan.json()).token },
  });
  expect(merged.status(), await merged.text()).toBe(200);
}

const dialog = (page: Page) => page.getByRole('alertdialog', { name: 'Merge two people' });
const editor = (page: Page) => page.getByRole('dialog', { name: 'Edit person' });
const card = (page: Page) => page.getByTestId('person-card');

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(request, otherRequest);
});

test.afterEach(async ({ page, request, otherRequest }) => {
  await page.setExtraHTTPHeaders({});
  await wipe(request, otherRequest);
});

test('a person’s page: choose the other, read the plan, swap, and merge', async ({ page, request }) => {
  const stamp = newStamp();
  const ada = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    emails: [{ value: `ada.${stamp}@example.com` }],
  });
  const abe = await person(request, {
    display_name: `e2e Abe ${stamp}`,
    company: `Initech ${stamp}`,
    emails: [{ value: `abe.${stamp}@example.com` }],
    phones: [{ value: '+1 555 010 7788' }],
  });
  const alone = await createProject(request, `${NODE} alone ${stamp}`);
  const both = await createProject(request, `${NODE} both ${stamp}`);
  await putOn(request, alone.id, abe, 'Lead');
  await putOn(request, both.id, ada, undefined, 'Since March.');
  await putOn(request, both.id, abe, 'Guest', 'Pays.');
  const minutes = await createNode(request, `${NODE} minutes ${stamp}`, { body: `Met ${token(abe)}.` });

  await page.goto(`/people/${ada.id}`);
  await expect(card(page).getByRole('heading', { name: ada.display_name })).toBeVisible();

  // Escape leaves both as they are.
  await card(page).getByRole('button', { name: 'Merge with another person' }).click();
  await expect(dialog(page).getByTestId('merge-heading')).toBeVisible();
  const search = dialog(page).getByRole('searchbox');
  await expect(search).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog(page)).toHaveCount(0);
  expect((await request.get(`/api/people/${abe.id}`)).status()).toBe(200);

  // The person kept is never offered as the other.
  await card(page).getByRole('button', { name: 'Merge with another person' }).click();
  await dialog(page).getByRole('searchbox').fill(stamp);
  const candidates = dialog(page).getByTestId('merge-candidate');
  await expect(candidates).toHaveCount(1);
  await expect(candidates).toContainText(abe.display_name);
  // What tells two people of one name apart is on the row.
  await expect(candidates).toContainText(`abe.${stamp}@example.com`);
  await candidates.click();

  // The plan: who is kept, who goes, and what the merge does.
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(ada.display_name);
  await expect(dialog(page).getByTestId('merge-gone')).toContainText(abe.display_name);
  const lines = dialog(page).getByTestId('merge-lines');
  await expect(lines).toContainText('Added to the kept person: company, 1 email, 1 phone.');
  await expect(lines).toContainText(
    `A line is added to the kept person’s notes: Merged in: name “${abe.display_name}”.`,
  );
  await expect(lines).toContainText('The other’s place on 1 node moves to the kept person.');
  // She has no role there, so his fills the blank: nothing is "not kept".
  await expect(lines).toContainText(`Both are on “${both.title}”: one entry is kept, with the notes of both.`);
  await expect(lines).not.toContainText('is not kept');
  await expect(lines).toContainText('The other is mentioned in 1 node you can open.');
  await expect(dialog(page)).toContainText('This can’t be undone');
  // The safe button has the keyboard.
  await expect(dialog(page).getByTestId('merge-back')).toBeFocused();

  // Swapping is a new plan, the other way round; and back.
  await dialog(page).getByTestId('merge-swap').click();
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(abe.display_name);
  await expect(dialog(page).getByTestId('merge-gone')).toContainText(ada.display_name);
  await dialog(page).getByTestId('merge-swap').click();
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(ada.display_name);
  await expect(lines).toContainText('Added to the kept person: company, 1 email, 1 phone.');

  await dialog(page).getByTestId('merge-confirm').click();
  await expect(page.getByText('Merged.', { exact: true })).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);

  // The page shows the person kept, with what the other had.
  await expect(page).toHaveURL(new RegExp(`/people/${ada.id}$`));
  await expect(card(page)).toContainText(`abe.${stamp}@example.com`);
  await expect(card(page)).toContainText(`Initech ${stamp}`);
  await expect(card(page)).toContainText(`Merged in: name “${abe.display_name}”.`);
  await expect(page.getByTestId('person-usages').getByRole('listitem')).toHaveCount(2);
  await expect(page.getByTestId('mentioned-in-row')).toHaveCount(1);

  // The other is gone, on every route; their memberships are the kept
  // person's, once per node.
  expect((await request.get(`/api/people/${abe.id}`)).status()).toBe(404);
  const members = await (await request.get(`/api/nodes/${both.id}/people`)).json();
  expect(members).toHaveLength(1);
  expect(members[0].person_id).toBe(ada.id);
  expect(members[0].role).toBe('Guest');
  expect(members[0].note).toBe('Since March.\n\nPays.');

  // No text was touched, and the old mention is a link to the kept person.
  const saved = await (await request.get(`/api/nodes/${minutes.id}`)).json();
  expect(saved.body).toBe(`Met ${token(abe)}.`);
  await page.goto(`/nodes/${minutes.id}`);
  const mention = page.locator('a.mention');
  await expect(mention).toHaveText(`@${ada.display_name}`);
  await expect(mention).toHaveAttribute('href', `/people/${ada.id}`);
  await mention.click();
  await expect(page).toHaveURL(new RegExp(`/people/${ada.id}$`));
});

test('merging the page’s own person into another lands on the one kept', async ({ page, request }) => {
  const stamp = newStamp();
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const abe = await person(request, { display_name: `e2e Abe ${stamp}` });

  // An edit of Ada left unfinished: this device keeps what was typed.
  await page.goto(`/people/${ada.id}`);
  await card(page).getByRole('button', { name: 'Edit person' }).click();
  await editor(page).getByRole('textbox', { name: 'Position' }).fill('Half typed');
  await page.goto('/people');
  await expect(page.locator('main')).toBeVisible();
  await page.getByRole('searchbox', { name: 'Search people' }).fill(abe.display_name);
  await page.getByTestId('person-row').getByRole('button', { name: abe.display_name }).click();
  await expect(page).toHaveURL(new RegExp(`/people/${abe.id}$`));

  await card(page).getByRole('button', { name: 'Merge with another person' }).click();
  await dialog(page).getByRole('searchbox').fill(ada.display_name);
  await dialog(page).getByTestId('merge-candidate').click();
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(abe.display_name);
  // Two bare records: nothing to add, and the line names neither.
  await expect(dialog(page).getByTestId('merge-lines')).toContainText(
    'Nothing to add: the kept person already has the other’s details.',
  );
  await dialog(page).getByTestId('merge-swap').click();
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(ada.display_name);
  await dialog(page).getByTestId('merge-confirm').click();

  await expect(page).toHaveURL(new RegExp(`/people/${ada.id}$`));
  await expect(card(page).getByRole('heading', { name: ada.display_name })).toBeVisible();
  // The edit from before the merge is of a card that no longer reads so:
  // it is not offered again.
  await expect(card(page)).toContainText('Merged in: name');
  await expect(editor(page)).toHaveCount(0);
  // Back does not land on the person who is gone.
  await page.goBack();
  await expect(page).toHaveURL(/\/people$/);
});

test('the notice over an edit offers the merge; going back keeps what was typed', async ({ page, request }) => {
  const stamp = newStamp();
  const ada = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    emails: [{ value: `ada.${stamp}@example.com` }],
  });
  const abe = await person(request, {
    display_name: `e2e Abe ${stamp}`,
    emails: [{ value: `abe.${stamp}@example.com` }],
  });

  await page.goto(`/people/${ada.id}`);
  await card(page).getByRole('button', { name: 'Edit person' }).click();
  await editor(page).getByRole('textbox', { name: 'Position' }).fill('Typed here');
  await editor(page).getByRole('button', { name: 'Add email' }).click();
  const second = editor(page).getByTestId('email-row').nth(1).getByRole('textbox', { name: 'email', exact: true });
  await second.fill(`abe.${stamp}@example.com`);
  await editor(page).getByRole('button', { name: 'Save' }).click();

  const refused = editor(page).getByTestId('email-taken');
  await expect(refused).toContainText('or merge the two into one');
  await expect(refused).toContainText('it does not save the other changes typed here');
  await refused.getByTestId('merge-with').click();

  // One dialog at a time: the merge takes the editor's place.
  await expect(editor(page)).toHaveCount(0);
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(ada.display_name);
  await expect(dialog(page).getByTestId('merge-gone')).toContainText(abe.display_name);
  // The asker said who: there is no one else to choose.
  await expect(dialog(page).getByTestId('merge-choose-again')).toHaveCount(0);

  // Going back returns to the edit, with what was typed.
  await dialog(page).getByTestId('merge-back').click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('Typed here');
  await expect(
    editor(page).getByTestId('email-row').nth(1).getByRole('textbox', { name: 'email', exact: true }),
  ).toHaveValue(`abe.${stamp}@example.com`);

  // Again, and this time the merge: what was typed is not saved by it.
  await editor(page).getByRole('button', { name: 'Save' }).click();
  await editor(page).getByTestId('email-taken').getByTestId('merge-with').click();
  await dialog(page).getByTestId('merge-confirm').click();
  await expect(page.getByText('Merged.', { exact: true })).toBeVisible();
  await expect(editor(page)).toHaveCount(0);
  await expect(card(page)).toContainText(`abe.${stamp}@example.com`);
  await expect(card(page)).not.toContainText('Typed here');
  expect((await request.get(`/api/people/${abe.id}`)).status()).toBe(404);
  // Nothing of the edit is offered again on a later visit.
  await page.reload();
  await expect(card(page).getByRole('heading', { name: ada.display_name })).toBeVisible();
  await expect(editor(page)).toHaveCount(0);
});

test('Data health: people who share an address are merged from the report', async ({ page, request }) => {
  const stamp = newStamp();
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const abe = await person(request, { display_name: `e2e Abe ${stamp}` });
  const cy = await person(request, { display_name: `e2e Cy ${stamp}` });
  // The API refuses a second person with an address someone has, so no
  // call can make the pair this card is for: only older data has one.
  // The report is served with these three as such a group, read from the
  // registry on every load; the fix runs against the real API.
  await page.route('**/api/data-health', async (route) => {
    const left: Person[] = await (await request.get(`/api/people?q=${stamp}`)).json();
    const ref = (p: Person) => ({ kind: 'person', id: p.id, label: p.display_name });
    const items = left.length < 2 ? [] : [{ subject: ref(left.find((p) => p.id === ada.id) ?? left[0]),
      related: left.filter((p) => p.id !== ada.id).map(ref), more: 0 }];
    await route.fulfill({ json: { rules: [{ rule: 'shared_email', count: items.length, items }] } });
  });

  await page.goto('/data-health');
  const rule = page.getByTestId('data-health-shared_email');
  await expect(rule.getByTestId('data-health-item')).toHaveCount(1);
  await expect(rule).toContainText('If they are one person, merge them.');
  await rule.getByTestId('fix-merge-people').click();

  // Two others share with her: the dialog asks which, by name.
  const among = dialog(page).getByTestId('merge-among');
  await expect(among.getByTestId('merge-candidate')).toHaveCount(2);
  await among.getByRole('button', { name: cy.display_name }).click();
  await expect(dialog(page).getByTestId('merge-kept')).toContainText(ada.display_name);
  await expect(dialog(page).getByTestId('merge-gone')).toContainText(cy.display_name);
  // Not the one meant: choose again.
  await dialog(page).getByTestId('merge-choose-again').click();
  await dialog(page).getByTestId('merge-among').getByRole('button', { name: abe.display_name }).click();
  await expect(dialog(page).getByTestId('merge-gone')).toContainText(abe.display_name);
  await dialog(page).getByTestId('merge-confirm').click();
  await expect(page.getByText('Merged.', { exact: true })).toBeVisible();

  // The report reads again: Ada and Cy are left, and with one other the
  // dialog opens on the plan.
  expect((await request.get(`/api/people/${abe.id}`)).status()).toBe(404);
  await expect(rule.getByTestId('data-health-item')).toHaveCount(1);
  await rule.getByTestId('fix-merge-people').click();
  await expect(dialog(page).getByTestId('merge-gone')).toContainText(cy.display_name);
  await expect(dialog(page).getByTestId('merge-among')).toHaveCount(0);
  await dialog(page).getByTestId('merge-back').click();
  await expect(dialog(page)).toHaveCount(0);
  // Going back leaves the fix ready to press again.
  await expect(rule.getByTestId('fix-merge-people')).toHaveAttribute('aria-disabled', 'false');
});

test('a plan that changed is shown again, and a refusal leaves nothing to press', async ({ page, request }) => {
  const stamp = newStamp();
  const ada = await person(request, { display_name: `e2e Ada ${stamp}`, notes: 'a'.repeat(6000) });
  const abe = await person(request, { display_name: `e2e Abe ${stamp}` });

  await page.goto(`/people/${ada.id}`);
  await card(page).getByRole('button', { name: 'Merge with another person' }).click();
  await dialog(page).getByRole('searchbox').fill(abe.display_name);
  await dialog(page).getByTestId('merge-candidate').click();
  await expect(dialog(page).getByTestId('merge-confirm')).toBeVisible();

  // The other is changed elsewhere while the plan is on screen: the
  // merge is not carried out, and the plan is shown as it is now.
  const edited = await request.put(`/api/people/${abe.id}`, {
    data: { display_name: abe.display_name, company: `Late ${stamp}`, emails: [] },
  });
  expect(edited.status()).toBe(200);
  await dialog(page).getByTestId('merge-confirm').click();
  await expect(dialog(page).getByTestId('merge-changed')).toBeVisible();
  await expect(dialog(page).getByTestId('merge-lines')).toContainText('Added to the kept person: company.');
  expect((await request.get(`/api/people/${abe.id}`)).status()).toBe(200);

  // Notes that are too long together: said, and there is no Merge button.
  const long = await request.put(`/api/people/${abe.id}`, {
    data: { display_name: abe.display_name, notes: 'b'.repeat(6000), emails: [] },
  });
  expect(long.status()).toBe(200);
  await dialog(page).getByTestId('merge-swap').click();
  await expect(dialog(page).getByTestId('merge-refusals')).toContainText('Together their notes are longer than');
  await expect(dialog(page).getByTestId('merge-confirm')).toHaveCount(0);
  await dialog(page).getByTestId('merge-back').click();
  for (const p of [ada, abe]) expect((await request.get(`/api/people/${p.id}`)).status()).toBe(200);
});

test('an edit from before a merge is refused, and says to reload', async ({ page, request }) => {
  const stamp = newStamp();
  const ada = await person(request, { display_name: `e2e Ada ${stamp}` });
  const abe = await person(request, { display_name: `e2e Abe ${stamp}`, company: `Initech ${stamp}` });

  await page.goto(`/people/${ada.id}`);
  await card(page).getByRole('button', { name: 'Edit person' }).click();
  await editor(page).getByRole('textbox', { name: 'Position' }).fill('From an old tab');

  // Meanwhile she is changed in another tab. This one's save would write
  // its older copy over that: refused, and it says what to do.
  const elsewhere = await request.put(`/api/people/${ada.id}`, {
    data: { display_name: ada.display_name, notes: `Elsewhere ${stamp}`, emails: [] },
  });
  expect(elsewhere.status()).toBe(200);
  await editor(page).getByRole('button', { name: 'Save' }).click();
  await expect(editor(page).getByRole('alert')).toContainText('Not saved. This person was changed somewhere else');
  await expect(editor(page).getByRole('alert')).toContainText('What you typed is kept on this device');
  expect((await (await request.get(`/api/people/${ada.id}`)).json()).notes).toBe(`Elsewhere ${stamp}`);
  // After the reload the form is offered again, and says she was changed
  // since. Saving from there is a choice made knowing it.
  await page.reload();
  await expect(editor(page).getByTestId('form-draft-recovery')).toContainText('has been changed');
  await editor(page).getByRole('button', { name: 'Save' }).click();
  await expect(editor(page)).toHaveCount(0);
  await expect(card(page)).toContainText('From an old tab');

  // The same with a merge in between.
  await card(page).getByRole('button', { name: 'Edit person' }).click();
  await editor(page).getByRole('textbox', { name: 'Position' }).fill('From an old tab, again');
  await mergeByApi(request, ada, abe);

  await editor(page).getByRole('button', { name: 'Save' }).click();
  const refused = editor(page).getByRole('alert');
  await expect(refused).toContainText('Not saved. This person was changed somewhere else');
  await expect(refused).toContainText('Reload the page');
  // The merge stands: the old tab wrote nothing over it.
  const now: Person & { company?: string } = await (await request.get(`/api/people/${ada.id}`)).json();
  expect(now.company).toBe(`Initech ${stamp}`);
  expect(now.position).toBe('From an old tab');

  // After a reload what was typed is offered on the person as they are.
  await page.reload();
  await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
  await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('From an old tab, again');
  await editor(page).getByRole('button', { name: 'Discard' }).click();

  // A tab still open on the one who went: their save finds nobody.
  const gone = await request.put(`/api/people/${abe.id}`, {
    data: { display_name: 'Back again', emails: [], expected_updated_at: abe.updated_at },
  });
  expect(gone.status()).toBe(404);
});

test('someone the node is shared with sees the kept person’s name on an old mention, as plain text', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = newStamp();
  const ada = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    emails: [{ value: `ada.${stamp}@example.com` }],
  });
  const abe = await person(request, { display_name: `e2e Abe ${stamp}` });
  const node = await createNode(request, `${NODE} shared ${stamp}`, { body: `Met ${token(abe)}.` });
  await shareNode(request, node.id, OTHER, 'editor');
  await mergeByApi(request, ada, abe);

  // What the API tells them: the kept person's name under the id their
  // text holds, and nothing else. Not the person's id as it is now, in
  // any field: they are not her owner.
  const theirs = await otherRequest.get(`/api/nodes/${node.id}`);
  const answer = await theirs.text();
  expect(JSON.parse(answer).mentions).toEqual([
    { person_id: abe.id, display_name: ada.display_name, mine: false },
  ]);
  expect(answer).not.toContain(ada.id);
  expect(answer).not.toContain('links_to');
  expect(answer).not.toContain('example.com');
  // Neither id is a way into the registry.
  for (const id of [ada.id, abe.id]) {
    expect((await otherRequest.get(`/api/people/${id}`)).status()).toBe(404);
    expect((await otherRequest.get(`/api/people/${id}/merge?from=${abe.id}`)).status()).not.toBe(200);
  }
  // A token of theirs with the old id names nobody new: it is her
  // mention that is real, not their typing.
  const planted = await createNode(otherRequest, `${NODE} planted ${stamp}`, { body: `Guess ${token(abe)}.` });
  expect((await (await otherRequest.get(`/api/nodes/${planted.id}`)).json()).mentions).toEqual([]);

  // The owner is told where the link leads.
  const mine = await (await request.get(`/api/nodes/${node.id}`)).json();
  expect(mine.mentions).toEqual([
    { person_id: abe.id, display_name: ada.display_name, mine: true, links_to: ada.id },
  ]);

  // On their page: the kept person's name, as plain text.
  await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
  await page.goto(`/nodes/${node.id}`);
  await expect(page.locator('span.mention')).toHaveText(`@${ada.display_name}`);
  await expect(page.locator('a.mention')).toHaveCount(0);
  await expect(page.locator('main')).not.toContainText('example.com');
});
