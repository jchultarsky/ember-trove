import { test, expect, createNode, shareNode, type APIRequestContext, type Page } from './fixtures';

/**
 * People (#147): the registry and a person's card, the People section on a
 * node (picker-first add, role, remove), and privacy. A sharee sees name and
 * role only; another user's person is a 404.
 */

type Person = { id: string; display_name: string; emails: { value: string }[] };
type Member = { id: string; person_id: string; display_name: string; role: string | null; note?: string; person?: unknown };

const OTHER_SUBJECT = 'e2e-other-user';

async function wipePeople(request: APIRequestContext, stamp: string) {
  const people: Person[] = await (await request.get(`/api/people?q=${encodeURIComponent(stamp)}`)).json();
  for (const p of people) await request.delete(`/api/people/${p.id}`);
}

/** The shared person editor: a dialog (desktop) or sheet (phone). */
const editor = (page: Page) => page.getByRole('dialog', { name: /New person|Edit person/ });

test('the registry: one editor for new and edit, and the duplicate warning', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const name = `e2e Ada ${stamp}`;
  const email = `ada.${stamp}@example.com`;
  await wipePeople(request, stamp);
  try {
    // Someone already has the email: saving it on a second person warns.
    const first = await request.post('/api/people', {
      data: { display_name: `e2e Ada original ${stamp}`, emails: [{ label: 'work', value: email, preferred: true }] },
    });
    expect(first.status()).toBe(201);

    // New person: the full editor straight away, name focused.
    await page.goto('/people');
    await page.getByRole('button', { name: 'New person' }).click();
    const dlg = editor(page);
    await expect(dlg).toBeVisible();
    await expect(dlg.getByRole('textbox', { name: 'Name' })).toBeFocused();
    await dlg.getByRole('textbox', { name: 'Name' }).fill(name);
    await dlg.getByRole('textbox', { name: 'Company' }).fill('Analytical Engines');
    await dlg.getByRole('button', { name: 'Add email' }).click();
    await dlg.getByRole('textbox', { name: 'email', exact: true }).fill(email);
    await dlg.getByRole('button', { name: 'Add phone' }).click();
    await dlg.getByRole('textbox', { name: 'phone', exact: true }).fill('+1 555 010 0000');
    await dlg.getByRole('button', { name: 'Save' }).click();
    // The duplicate warning shows inside the editor.
    const warning = dlg.getByTestId('duplicate-warning');
    await expect(warning).toContainText(`e2e Ada original ${stamp}`);
    await warning.getByRole('button', { name: 'Save anyway' }).click();
    await expect(dlg).toHaveCount(0);
    // Saving returns to the registry (#154): the new row is there,
    // highlighted, with a toast. Its card is a row click away.
    await expect(page).toHaveURL(/\/people$/);
    await expect(page.getByText('Person added.')).toBeVisible();
    const row = page.getByTestId('person-row').filter({ hasText: name });
    await expect(row).toHaveAttribute('data-new', 'true');
    // Another New starts from a blank form (the burst case).
    await page.getByRole('button', { name: 'New person' }).click();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue('');
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
    // Click the name: the row's centre can land on its email link or copy
    // button, which stop propagation.
    await row.getByText(name, { exact: true }).click();
    await expect(page).toHaveURL(/\/people\/[0-9a-f-]{36}$/);

    // The card: contact values are actions.
    const card = page.getByTestId('person-card');
    await expect(card.getByRole('heading', { name })).toBeVisible();
    await expect(card.getByRole('link', { name: email })).toHaveAttribute('href', `mailto:${email}`);
    await expect(card.getByRole('link', { name: '+1 555 010 0000' })).toHaveAttribute('href', 'tel:+15550100000');

    // Edit opens the same editor on the saved values.
    await card.getByRole('button', { name: 'Edit' }).click();
    await expect(editor(page).getByRole('heading', { name: 'Edit person' })).toBeVisible();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(name);
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('Analytical Engines');
    // Position says it's the person's, everywhere (#182).
    await expect(editor(page).getByRole('textbox', { name: 'Position' }))
      .toHaveAccessibleDescription('Their job title \u2014 stays with the person on every node.');
    await editor(page).getByRole('textbox', { name: 'Position' }).fill('Analyst');
    // Ctrl/⌘+Enter saves from any field. The person keeps their own email,
    // which matches the original's, so this is a confirmed duplicate: the
    // warning shows again, and "Save anyway" saves.
    await page.keyboard.press('ControlOrMeta+Enter');
    await editor(page).getByTestId('duplicate-warning').getByRole('button', { name: 'Save anyway' }).click();
    await expect(editor(page)).toHaveCount(0);
    await expect(card).toContainText('Analyst');

    // Re-saving a person whose email no one else has never warns.
    const original: Person = await first.json();
    const resave = await request.put(`/api/people/${original.id}`, {
      data: { display_name: original.display_name, emails: [{ label: 'work', value: `solo.${stamp}@example.com` }] },
    });
    expect(resave.status()).toBe(200);
    const again = await request.put(`/api/people/${original.id}`, {
      data: { display_name: original.display_name, emails: [{ label: 'work', value: `solo.${stamp}@example.com` }] },
    });
    expect(again.status()).toBe(200);

    // The registry search finds them by email.
    await page.goto('/people');
    await page.getByRole('searchbox', { name: 'Search people' }).fill(`ada.${stamp}`);
    await expect(page.getByTestId('person-row')).toHaveCount(1);
  } finally {
    await wipePeople(request, stamp);
  }
});

test('the editor: Cancel discards, Escape discards with Undo', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
  try {
    const p: Person = await (await request.post('/api/people', { data: { display_name: `e2e Mary ${stamp}` } })).json();
    await page.goto(`/people/${p.id}`);
    const card = page.getByTestId('person-card');
    await card.getByRole('button', { name: 'Edit' }).click();
    await editor(page).getByRole('textbox', { name: 'Company' }).fill('Somerville');
    // Escape with a change: closed, with "Changes discarded · Undo".
    await page.keyboard.press('Escape');
    await expect(editor(page)).toHaveCount(0);
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('Somerville');
    // Cancel discards outright, and the next Edit starts from the saved values.
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
    await expect(editor(page)).toHaveCount(0);
    await card.getByRole('button', { name: 'Edit' }).click();
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('');
    expect(new URL(page.url()).pathname).toBe(`/people/${p.id}`);
  } finally {
    await wipePeople(request, stamp);
  }
});

test('a reload keeps the open dialog\'s input, and only a dirty one (#170)', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
  try {
    // New person: typed, then the page reloads (what the update banner does).
    await page.goto('/people');
    await page.getByRole('button', { name: 'New person' }).click();
    await editor(page).getByRole('textbox', { name: 'Name' }).fill(`e2e Grace ${stamp}`);
    await editor(page).getByRole('textbox', { name: 'Company' }).fill('Navy');
    await page.reload();
    await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(`e2e Grace ${stamp}`);
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('Navy');
    // Discard ends it: the next load opens nothing.
    await editor(page).getByRole('button', { name: 'Discard' }).click();
    await expect(editor(page)).toHaveCount(0);
    await page.reload();
    await expect(page.locator('main')).toBeVisible();
    await expect(page.getByRole('button', { name: 'New person' })).toBeVisible();
    await expect(editor(page)).toHaveCount(0);

    // Edit: the draft survives, and says so when the card changed since.
    const p: Person = await (await request.post('/api/people', { data: { display_name: `e2e Hopper ${stamp}` } })).json();
    await page.goto(`/people/${p.id}`);
    const card = page.getByTestId('person-card');
    await card.getByRole('button', { name: 'Edit' }).click();
    await editor(page).getByRole('textbox', { name: 'Position' }).fill('Rear Admiral');
    await request.put(`/api/people/${p.id}`, { data: { display_name: `e2e Hopper ${stamp}`, company: 'Harvard' } });
    await page.reload();
    await expect(editor(page).getByTestId('form-draft-recovery')).toContainText('changed since');
    await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('Rear Admiral');
    // Saving drops the draft: a reload opens nothing.
    await editor(page).getByRole('button', { name: 'Save' }).click();
    await expect(editor(page)).toHaveCount(0);
    await page.reload();
    await expect(card).toBeVisible();
    await expect(editor(page)).toHaveCount(0);

    // A dialog nobody typed in keeps nothing.
    await card.getByRole('button', { name: 'Edit' }).click();
    await expect(editor(page)).toBeVisible();
    await page.reload();
    await expect(card).toBeVisible();
    await expect(editor(page)).toHaveCount(0);
  } finally {
    await wipePeople(request, stamp);
  }
});

test('on a phone the editor is a full-screen sheet with Save in reach', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
  await page.setViewportSize({ width: 375, height: 812 });
  try {
    await page.goto('/people');
    await expect(page.locator('main')).toBeVisible();
    await page.getByRole('button', { name: 'New person' }).click();
    const dlg = editor(page);
    await expect(dlg).toBeVisible();
    const box = await dlg.boundingBox();
    expect(box).toMatchObject({ x: 0, y: 0, width: 375, height: 812 });
    // Contact rows stack: the label sits above the value, not beside it.
    await dlg.getByRole('button', { name: 'Add phone' }).click();
    const label = await dlg.getByRole('combobox', { name: 'phone label' }).boundingBox();
    const value = await dlg.getByRole('textbox', { name: 'phone', exact: true }).boundingBox();
    expect(label!.y + label!.height).toBeLessThanOrEqual(value!.y);
    // Nothing scrolls sideways, and Save is on screen.
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);
    const save = await dlg.getByRole('button', { name: 'Save' }).boundingBox();
    expect(save!.y + save!.height).toBeLessThanOrEqual(812);
    await dlg.getByRole('textbox', { name: 'Name' }).fill(`e2e Katherine ${stamp}`);
    await dlg.getByRole('button', { name: 'Save' }).click();
    await expect(dlg).toHaveCount(0);
    await expect(page).toHaveURL(/\/people$/);
    await expect(page.getByTestId('person-row').filter({ hasText: `e2e Katherine ${stamp}` })).toBeVisible();
  } finally {
    await wipePeople(request, stamp);
  }
});

test('a node: add from the registry or create inline, with a role, remove', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const node = await createNode(request, `e2e people project ${stamp}`, { node_type: 'project' });
  await wipePeople(request, stamp);
  try {
    const existing: Person = await (
      await request.post('/api/people', { data: { display_name: `e2e Grace ${stamp}`, company: 'Navy' } })
    ).json();
    await request.post('/api/people', { data: { display_name: `e2e Hopper ${stamp}` } });

    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-people');
    await section.getByRole('button', { name: /People/ }).first().click();
    await section.getByRole('button', { name: 'Add person' }).click();
    const picker = section.getByRole('combobox', { name: 'Find or create a person' });
    await expect(picker).toBeFocused();

    // From the registry: picking asks for the optional Role and Note, then
    // attaches with them in one motion (#162).
    await picker.fill(`Grace ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText(`e2e Grace ${stamp}`);
    await page.keyboard.press('Enter');
    const attach = section.getByTestId('person-attach');
    await expect(attach).toContainText(`e2e Grace ${stamp}`);
    // The Role field is a combobox (with suggestions), not a textbox.
    await expect(attach.getByRole('combobox', { name: 'Role' })).toBeFocused();
    await attach.getByRole('combobox', { name: 'Role' }).fill('Approver');
    await page.keyboard.press('Enter');
    const rows = section.getByTestId('node-person');
    await expect(rows).toHaveCount(1);
    await expect(rows.first().getByTestId('member-role')).toHaveText('Approver');

    // Create inline when there's no match: the same editor, name filled in,
    // and saving adds the new person here.
    await section.getByRole('button', { name: 'Add person' }).click();
    await section.getByRole('combobox', { name: 'Find or create a person' }).fill(`e2e Linus ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText('Create');
    await page.keyboard.press('Enter');
    const dlg = page.getByRole('dialog', { name: 'New person' });
    await expect(dlg.getByRole('textbox', { name: 'Name' })).toHaveValue(`e2e Linus ${stamp}`);
    await dlg.getByRole('textbox', { name: 'Company' }).fill('Kernel');
    // The membership's own block, apart from the person's fields.
    const onNode = dlg.getByRole('group', { name: 'On this node' });
    // Role says it's this node's only (#182).
    await expect(onNode.getByRole('combobox', { name: 'Role' }))
      .toHaveAccessibleDescription('Their part in this node only.');
    await onNode.getByRole('combobox', { name: 'Role' }).fill('Maintainer');
    await onNode.getByRole('textbox', { name: 'Note' }).fill('owns the kernel');
    await dlg.getByRole('button', { name: 'Save' }).click();
    await expect(dlg).toHaveCount(0);
    await expect(rows).toHaveCount(2);
    const linus = rows.filter({ hasText: `e2e Linus ${stamp}` });
    await expect(linus).toContainText('Kernel');
    await expect(linus.getByTestId('member-role')).toHaveText('Maintainer');
    await expect(linus).toContainText('owns the kernel');

    // Back from the attach step returns to the search, nothing attached.
    await section.getByRole('button', { name: 'Add person' }).click();
    await section.getByRole('combobox', { name: 'Find or create a person' }).fill(`Hopper ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText(`e2e Hopper ${stamp}`);
    await page.keyboard.press('Enter');
    await section.getByTestId('person-attach').getByRole('button', { name: 'Back' }).click();
    await expect(section.getByRole('combobox', { name: 'Find or create a person' })).toBeFocused();
    await section.getByRole('button', { name: 'Cancel' }).click();
    await expect(rows).toHaveCount(2);

    // The row editor still changes the role later.
    const grace = rows.filter({ hasText: `e2e Grace ${stamp}` });
    await grace.getByRole('button', { name: 'Edit role and note' }).click();
    await grace.getByRole('combobox', { name: 'Role' }).fill('Sponsor');
    await grace.getByRole('button', { name: 'Save' }).click();
    await expect(grace.getByTestId('member-role')).toHaveText('Sponsor');

    // "Used in" on the person's page.
    const used: { node_id: string; role: string | null }[] = await (
      await request.get(`/api/people/${existing.id}/usages`)
    ).json();
    expect(used).toEqual([expect.objectContaining({ node_id: node.id, role: 'Sponsor' })]);

    // Removing a membership never deletes the person.
    await grace.getByRole('button', { name: 'Remove from this node' }).click();
    await expect(rows).toHaveCount(1);
    expect((await request.get(`/api/people/${existing.id}`)).status()).toBe(200);
  } finally {
    await wipePeople(request, stamp);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('the person picker: an accessibility press picks an option, mouse picks once (#174)', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const node = await createNode(request, `e2e people ax ${stamp}`, { node_type: 'project' });
  await wipePeople(request, stamp);
  try {
    await request.post('/api/people', { data: { display_name: `e2e Barbara ${stamp}` } });
    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-people');
    await section.getByRole('button', { name: /People/ }).first().click();
    await section.getByRole('button', { name: 'Add person' }).click();
    const picker = section.getByRole('combobox', { name: 'Find or create a person' });
    await picker.fill(`Barbara ${stamp}`);
    await expect(picker).toHaveAttribute('aria-expanded', 'true');
    const option = section.getByRole('option', { name: new RegExp(`e2e Barbara ${stamp}`) });
    // VoiceOver / AXPress / element.click(): a bare click, detail 0, with
    // no mousedown before it. It used to do nothing.
    await option.evaluate((el) => (el as HTMLElement).click());
    const attach = section.getByTestId('person-attach');
    await expect(attach).toContainText(`e2e Barbara ${stamp}`);
    await attach.getByRole('button', { name: 'Back' }).click();

    // A real mouse click still commits on mousedown, and only once: the
    // attach step shows, not the editor or a second step.
    await expect(picker).toBeFocused();
    await section.getByRole('option', { name: new RegExp(`e2e Barbara ${stamp}`) }).click();
    await expect(attach).toContainText(`e2e Barbara ${stamp}`);
    await expect(page.getByRole('dialog', { name: 'New person' })).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
    await wipePeople(request, stamp);
  }
});

test('a node: the attach step and block left empty, Escape, and no block in the registry (#162)', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const node = await createNode(request, `e2e people blank ${stamp}`, { node_type: 'project' });
  await wipePeople(request, stamp);
  try {
    for (const name of [`e2e Grace ${stamp}`, `e2e Ada ${stamp}`]) {
      expect((await request.post('/api/people', { data: { display_name: name } })).status()).toBe(201);
    }
    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-people');
    await section.getByRole('button', { name: /People/ }).first().click();
    const rows = section.getByTestId('node-person');
    const picker = section.getByRole('combobox', { name: 'Find or create a person' });

    // Pick existing, step left empty: Enter attaches exactly as before #162.
    await section.getByRole('button', { name: 'Add person' }).click();
    await picker.fill(`Grace ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText(`e2e Grace ${stamp}`);
    await page.keyboard.press('Enter');
    const attach = section.getByTestId('person-attach');
    await expect(attach.getByRole('combobox', { name: 'Role' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(rows).toHaveCount(1);
    await expect(rows.getByTestId('member-role')).toHaveCount(0);

    // Escape steps back to the search, with the query kept; nothing attached.
    await section.getByRole('button', { name: 'Add person' }).click();
    await picker.fill(`Ada ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText(`e2e Ada ${stamp}`);
    await page.keyboard.press('Enter');
    await expect(attach.getByRole('combobox', { name: 'Role' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(picker).toBeFocused();
    await expect(picker).toHaveValue(`Ada ${stamp}`);
    await expect(rows).toHaveCount(1);
    await section.getByRole('button', { name: 'Cancel' }).click();

    // Create from the section, "On this node" left empty: no role either.
    await section.getByRole('button', { name: 'Add person' }).click();
    await picker.fill(`e2e Linus ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText('Create');
    await page.keyboard.press('Enter');
    const dlg = page.getByRole('dialog', { name: 'New person' });
    await expect(dlg.getByRole('group', { name: 'On this node' })).toBeVisible();
    await dlg.getByRole('button', { name: 'Save' }).click();
    await expect(dlg).toHaveCount(0);
    await expect(rows).toHaveCount(2);
    await expect(rows.getByTestId('member-role')).toHaveCount(0);

    // Both memberships are role- and note-less in the API too.
    const members: Member[] = await (await request.get(`/api/nodes/${node.id}/people`)).json();
    expect(members.map((m) => [m.display_name, m.role, m.note ?? null]).sort()).toEqual([
      [`e2e Grace ${stamp}`, null, null],
      [`e2e Linus ${stamp}`, null, null],
    ]);

    // What was typed in the block doesn't carry over to the next opening.
    await section.getByRole('button', { name: 'Add person' }).click();
    await picker.fill(`e2e Edsger ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText('Create');
    await page.keyboard.press('Enter');
    const onNode = dlg.getByRole('group', { name: 'On this node' });
    await onNode.getByRole('combobox', { name: 'Role' }).fill('Reviewer');
    await dlg.getByRole('button', { name: 'Cancel' }).click();
    await expect(dlg).toHaveCount(0);
    // Cancelling the editor leaves the picker open, query and all.
    await expect(picker).toHaveValue(`e2e Edsger ${stamp}`);
    await picker.click();
    await expect(section.getByRole('option', { selected: true })).toContainText('Create');
    await page.keyboard.press('Enter');
    await expect(onNode.getByRole('combobox', { name: 'Role' })).toHaveValue('');
    await dlg.getByRole('button', { name: 'Cancel' }).click();

    // The registry's own editor isn't attaching to a node: no block.
    await page.goto('/people');
    await page.getByRole('button', { name: 'New person' }).click();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toBeVisible();
    await expect(editor(page).getByTestId('membership-fields')).toHaveCount(0);
  } finally {
    await wipePeople(request, stamp);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('the Role combobox saves what was typed; a suggestion only when chosen (#171)', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const node = await createNode(request, `e2e people role ${stamp}`, { node_type: 'project' });
  await wipePeople(request, stamp);
  try {
    const mk = async (name: string): Promise<Person> =>
      (await request.post('/api/people', { data: { display_name: name } })).json();
    const grace = await mk(`e2e Grace ${stamp}`);
    await mk(`e2e Ada ${stamp}`);
    // An existing role that differs from the one we'll type only in casing.
    const seeded = await request.post(`/api/nodes/${node.id}/people`, {
      data: { person_id: grace.id, role: `qa${stamp}` },
    });
    expect(seeded.ok()).toBeTruthy();

    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-people');
    await section.getByRole('button', { name: /People/ }).first().click();
    const rows = section.getByTestId('node-person');
    await expect(rows).toHaveCount(1);

    // Typed "QA…" with "qa…" suggested: Enter saves the typed casing.
    await section.getByRole('button', { name: 'Add person' }).click();
    await section.getByRole('combobox', { name: 'Find or create a person' }).fill(`Ada ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText(`e2e Ada ${stamp}`);
    await page.keyboard.press('Enter');
    const attach = section.getByTestId('person-attach');
    const role = attach.getByRole('combobox', { name: 'Role' });
    await expect(role).toBeFocused();
    await role.fill(`QA${stamp}`);
    await expect(attach.getByRole('option', { name: `qa${stamp}` })).toBeVisible();
    await expect(attach.getByRole('option', { selected: true })).toHaveCount(0);
    await page.keyboard.press('Enter');
    const ada = rows.filter({ hasText: `e2e Ada ${stamp}` });
    await expect(ada.getByTestId('member-role')).toHaveText(`QA${stamp}`);

    // Choosing the suggestion (↓ then Enter) shows it in the field first.
    await ada.getByRole('button', { name: 'Edit role and note' }).click();
    const edit = ada.getByRole('combobox', { name: 'Role' });
    await edit.fill(`QA${stamp}`);
    await page.keyboard.press('ArrowDown');
    await expect(ada.getByRole('option', { selected: true })).toHaveText(`qa${stamp}`);
    await page.keyboard.press('Enter');
    await expect(edit).toHaveValue(`qa${stamp}`);
    await expect(ada.getByRole('listbox')).toBeHidden();

    // Select-all replaces the text instead of appending to it.
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.type(`Lead${stamp}`);
    await expect(edit).toHaveValue(`Lead${stamp}`);
    await ada.getByRole('button', { name: 'Save' }).click();
    await expect(ada.getByTestId('member-role')).toHaveText(`Lead${stamp}`);
  } finally {
    await wipePeople(request, stamp);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('contact labels suggest common values; a custom label saves as typed (#183)', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
  try {
    await page.goto('/people');
    await expect(page.locator('main')).toBeVisible();
    await page.getByRole('button', { name: 'New person' }).click();
    const dlg = editor(page);
    await expect(dlg).toBeVisible();
    await dlg.getByRole('textbox', { name: 'Name' }).fill(`e2e Labels ${stamp}`);

    // Phone: "o" offers the starter labels; ↓ then Enter picks one.
    await dlg.getByRole('button', { name: 'Add phone' }).click();
    const label = dlg.getByRole('combobox', { name: 'phone label' });
    await label.fill('o');
    const options = dlg.getByRole('listbox', { name: 'phone label suggestions' }).getByRole('option');
    await expect(options).toHaveText(['office', 'mobile', 'home']);
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await expect(label).toHaveValue('office');
    await dlg.getByRole('textbox', { name: 'phone', exact: true }).fill('+1 555 010 0001');

    // Email: a custom label is kept verbatim, nothing chosen for it.
    await dlg.getByRole('button', { name: 'Add email' }).click();
    await dlg.getByRole('combobox', { name: 'email label' }).fill('Work');
    await dlg.getByRole('textbox', { name: 'email', exact: true }).fill(`labels.${stamp}@example.com`);

    await dlg.getByRole('button', { name: 'Save' }).click();
    await expect(dlg).toHaveCount(0);
    const [saved]: { phones: { label: string }[]; emails: { label: string }[] }[] =
      await (await request.get(`/api/people?q=${encodeURIComponent(stamp)}`)).json();
    expect(saved.phones.map((p) => p.label)).toEqual(['office']);
    expect(saved.emails.map((e) => e.label)).toEqual(['Work']);
  } finally {
    await wipePeople(request, stamp);
  }
});

test('privacy: a sharee sees name and role only; another user\'s person is a 404', async ({ request, otherRequest }) => {
  const stamp = `${Date.now()}`;
  const node = await createNode(request, `e2e people privacy ${stamp}`, { node_type: 'project' });
  await wipePeople(request, stamp);
  try {
    const ada: Person = await (
      await request.post('/api/people', {
        data: { display_name: `e2e Ada ${stamp}`, emails: [{ value: `ada.${stamp}@example.com` }], notes: 'private' },
      })
    ).json();
    const added = await request.post(`/api/nodes/${node.id}/people`, {
      data: { person_id: ada.id, role: 'Sponsor', note: 'met at kickoff' },
    });
    expect(added.status()).toBe(201);
    await shareNode(request, node.id, OTHER_SUBJECT, 'editor');

    // The owner sees the card and the note.
    const mine: Member[] = await (await request.get(`/api/nodes/${node.id}/people`)).json();
    expect(mine[0].note).toBe('met at kickoff');
    expect(mine[0].person).toBeTruthy();

    // The sharee: name and role, and nothing else, not even in the raw body.
    const resp = await otherRequest.get(`/api/nodes/${node.id}/people`);
    expect(resp.status()).toBe(200);
    const raw = await resp.text();
    const theirs: Member[] = JSON.parse(raw);
    expect(theirs).toEqual([{ id: mine[0].id, person_id: ada.id, display_name: `e2e Ada ${stamp}`, role: 'Sponsor' }]);
    expect(raw).not.toContain('example.com');
    expect(raw).not.toContain('kickoff');

    // The person itself is invisible to them, and an editor can't change the node's people.
    expect((await otherRequest.get(`/api/people/${ada.id}`)).status()).toBe(404);
    expect(
      (await otherRequest.delete(`/api/nodes/${node.id}/people/${mine[0].id}`)).status(),
    ).toBe(403);
  } finally {
    await wipePeople(request, stamp);
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('global search finds people: the search page, the header dropdown and the palette', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const name = `e2e Hedy ${stamp}`;
  await wipePeople(request, stamp);
  try {
    const hedy: Person = await (
      await request.post('/api/people', { data: { display_name: name, company: 'Frequency Hopping' } })
    ).json();

    // The search page: a People group, which hides under a node-only filter.
    await page.goto('/tasks/my-day');
    await expect(page.locator('main')).toBeVisible();
    const bar = page.getByPlaceholder('Search\u2026 (Enter)');
    await bar.fill(`Hedy ${stamp}`);

    // The header dropdown lists the person; clicking opens their card.
    const fromBar = page.getByTestId('search-bar-person').filter({ hasText: name });
    await expect(fromBar).toBeVisible();
    await bar.press('Enter');
    await expect(page).toHaveURL(/\/search$/);
    const group = page.getByTestId('search-people');
    await expect(group.getByRole('button', { name: new RegExp(name) })).toBeVisible();
    await group.getByRole('button', { name: new RegExp(name) }).click();
    await expect(page).toHaveURL(new RegExp(`/people/${hedy.id}$`));

    // The palette: the person is an action, after node titles and before commands.
    await page.keyboard.press('ControlOrMeta+k');
    const input = page.getByPlaceholder('Search nodes, or type a new title\u2026');
    await expect(input).toBeFocused();
    await input.fill(`Hedy ${stamp}`);
    const item = page.getByRole('dialog').getByRole('button', { name: new RegExp(name) });
    await expect(item).toBeVisible();
    await item.click();
    await expect(page).toHaveURL(new RegExp(`/people/${hedy.id}$`));
  } finally {
    await wipePeople(request, stamp);
  }
});

