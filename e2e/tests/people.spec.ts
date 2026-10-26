import { test, expect, createNode, shareNode, type APIRequestContext } from './fixtures';

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

test('the registry: create a person, fill the card, and the duplicate warning', async ({ page, request }) => {
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

    await page.goto('/people');
    await page.getByRole('button', { name: 'New person' }).click();
    await page.getByRole('textbox', { name: "New person's name" }).fill(name);
    await page.getByRole('button', { name: 'Create' }).click();
    await expect(page).toHaveURL(/\/people\/[0-9a-f-]{36}$/);
    await expect(page.getByTestId('person-card').getByRole('heading', { name })).toBeVisible();

    // Fill in the card.
    await page.getByRole('button', { name: 'Edit' }).click();
    const form = page.getByTestId('person-form');
    await form.getByRole('button', { name: '+ Add email' }).click();
    await form.getByRole('textbox', { name: 'email', exact: true }).fill(email);
    await form.getByRole('button', { name: '+ Add phone' }).click();
    await form.getByRole('textbox', { name: 'phone', exact: true }).fill('+1 555 010 0000');
    await form.getByRole('button', { name: 'Save' }).click();
    const warning = page.getByTestId('duplicate-warning');
    await expect(warning).toContainText(`e2e Ada original ${stamp}`);
    await warning.getByRole('button', { name: 'Save anyway' }).click();

    // Contact values are actions.
    const card = page.getByTestId('person-card');
    await expect(card.getByRole('link', { name: email })).toHaveAttribute('href', `mailto:${email}`);
    await expect(card.getByRole('link', { name: '+1 555 010 0000' })).toHaveAttribute('href', 'tel:+15550100000');

    // Re-saving a person with their own email isn't a duplicate of
    // themselves (here: the original, whose email no one else had before).
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
    // The original moved to another address, so one match is left.
    await expect(page.getByTestId('person-row')).toHaveCount(1);
  } finally {
    await wipePeople(request, stamp);
  }
});

test('a node: add from the registry or create inline, set a role, remove', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  const node = await createNode(request, `e2e people project ${stamp}`, { node_type: 'project' });
  await wipePeople(request, stamp);
  try {
    const existing: Person = await (
      await request.post('/api/people', { data: { display_name: `e2e Grace ${stamp}`, company: 'Navy' } })
    ).json();

    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-people');
    await section.getByRole('button', { name: /People/ }).first().click();
    await section.getByRole('button', { name: 'Add person' }).click();
    const picker = section.getByRole('combobox', { name: 'Find or create a person' });
    await expect(picker).toBeFocused();

    // From the registry.
    await picker.fill(`Grace ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText(`e2e Grace ${stamp}`);
    await page.keyboard.press('Enter');
    const rows = section.getByTestId('node-person');
    await expect(rows).toHaveCount(1);

    // Create inline when there's no match.
    await section.getByRole('button', { name: 'Add person' }).click();
    await section.getByRole('combobox', { name: 'Find or create a person' }).fill(`e2e Linus ${stamp}`);
    await expect(section.getByRole('option', { selected: true })).toContainText('Create');
    await page.keyboard.press('Enter');
    await expect(rows).toHaveCount(2);

    // Role on the membership.
    const grace = rows.filter({ hasText: `e2e Grace ${stamp}` });
    await grace.getByRole('button', { name: 'Edit role and note' }).click();
    // An input with a datalist is a combobox, not a textbox.
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

