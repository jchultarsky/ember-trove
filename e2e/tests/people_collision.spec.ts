import { test, expect, createNode, deleteNodesByPrefix, type APIRequestContext, type Page } from './fixtures';

/**
 * What the app offers when an email address already belongs to someone
 * (S8b, #357; ONTOLOGY §7.1: "open the existing record or merge into
 * it"). A new person who collides can be added to the existing one for
 * review: their own values are kept, blanks are filled, lists grow, and
 * nothing is saved until Save. Backing out loses nothing.
 *
 * On a node the notice puts the person who has the address there instead,
 * with the Role and Note typed in the dialog.
 *
 * People are per account, so one identity is enough. Every person's name
 * holds `MARK`; `wipePeople` removes them all before and after, so an
 * attempt that crashed leaves nothing for its retry.
 */

const MARK = 'collide8b';
const newStamp = () => `${MARK}${Date.now()}`;

type Point = { label?: string; value: string; preferred?: boolean };
type Person = {
  id: string;
  display_name: string;
  company: string | null;
  position: string | null;
  emails: Point[];
  phones: Point[];
  notes: string | null;
};

async function wipePeople(request: APIRequestContext) {
  const people: Person[] = await (await request.get(`/api/people?q=${MARK}`)).json();
  for (const p of people) await request.delete(`/api/people/${p.id}`);
}

async function person(request: APIRequestContext, data: Record<string, unknown>): Promise<Person> {
  const made = await request.post('/api/people', { data });
  expect(made.status(), await made.text()).toBe(201);
  return made.json();
}

const fetched = async (request: APIRequestContext, id: string): Promise<Person> =>
  (await request.get(`/api/people/${id}`)).json();

const editor = (page: Page) => page.getByRole('dialog', { name: /New person|Edit person/ });
const emailRows = (page: Page) => editor(page).getByTestId('email-row');
const phoneRows = (page: Page) => editor(page).getByTestId('phone-row');
const emailBox = (page: Page, i: number) =>
  emailRows(page).nth(i).getByRole('textbox', { name: 'email', exact: true });

/** Opens "New person" and types a name. */
async function newPerson(page: Page, name: string) {
  await page.goto('/people');
  await page.getByRole('button', { name: 'New person' }).click();
  await expect(editor(page).getByRole('textbox', { name: 'Name' })).toBeFocused();
  await editor(page).getByRole('textbox', { name: 'Name' }).fill(name);
}

async function addEmail(page: Page, value: string) {
  const n = await emailRows(page).count();
  await editor(page).getByRole('button', { name: 'Add email' }).click();
  await emailBox(page, n).fill(value);
}

async function addPhone(page: Page, value: string) {
  const n = await phoneRows(page).count();
  await editor(page).getByRole('button', { name: 'Add phone' }).click();
  await phoneRows(page).nth(n).getByRole('textbox', { name: 'phone', exact: true }).fill(value);
}

/** Saves, and returns the refusal's notice. */
async function saveRefused(page: Page) {
  await editor(page).getByRole('button', { name: 'Save' }).click();
  const taken = editor(page).getByTestId('email-taken');
  await expect(taken).toContainText('Not saved.');
  // The notice that says why stays in view (focusing a row used to
  // scroll it away, 3.30.0), and so does the keyboard: it is on the
  // marked row when that fits under the notice, and on the notice if not.
  await expect(taken).toBeInViewport({ ratio: 1 });
  await expect(
    editor(page).locator('input[aria-invalid="true"]:focus, [role="alert"]:focus'),
  ).toBeInViewport({ ratio: 1 });
  return taken;
}

const carryButton = (taken: ReturnType<Page['getByTestId']>, theirs: Person) =>
  taken.getByRole('button', { name: `Add what I typed to them: ${theirs.display_name}` });

/** Opens their editor, types a position and leaves: this device keeps that edit (#170). */
async function keepAnEditOf(page: Page, theirs: Person, position: string) {
  await page.goto(`/people/${theirs.id}`);
  await page.getByRole('button', { name: 'Edit person' }).click();
  await editor(page).getByRole('textbox', { name: 'Position' }).fill(position);
  await page.goto('/people');
  await expect(page.getByRole('button', { name: 'New person' })).toBeVisible();
}

type Fixture = { stamp: string; theirs: Person; email: string; typedName: string };

/** A person who has an address, and a new person typed with it and more. */
async function collide(page: Page, request: APIRequestContext, stamp: string): Promise<Fixture> {
  const email = `ada.${stamp}@example.com`;
  const theirs = await person(request, {
    display_name: `e2e Ada ${stamp}`,
    company: 'Analytical Engines',
    emails: [{ label: 'work', value: email, preferred: true }],
    phones: [{ label: 'mobile', value: '+1 (555) 010-0000', preferred: true }],
  });
  const typedName = `e2e A. L. ${stamp}`;
  await newPerson(page, typedName);
  await editor(page).getByRole('textbox', { name: 'Company' }).fill('Engines Ltd');
  await editor(page).getByRole('textbox', { name: 'Position' }).fill('Analyst');
  // Their address in another case, and one they don't have.
  await addEmail(page, email.toUpperCase());
  await addEmail(page, `ada.home.${stamp}@example.com`);
  // Their number in another format, and one they don't have.
  await addPhone(page, '+1 555 010 0000');
  await addPhone(page, '+1 555 010 9999');
  await editor(page).getByRole('textbox', { name: 'Notes' }).fill('Prefers email.');
  return { stamp, theirs, email, typedName };
}

/** Clicks "add what I typed" for `theirs` and waits for their review. */
async function carryOver(page: Page, taken: ReturnType<Page['getByTestId']>, theirs: Person) {
  await carryButton(taken, theirs).click();
  await expect(page).toHaveURL(new RegExp(`/people/${theirs.id}$`));
  const review = editor(page).getByTestId('carry-review');
  await expect(review).toBeVisible();
  return review;
}

test('what was typed is added to the person who has the address, reviewed, then saved', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs, email, typedName } = await collide(page, request, stamp);
    const taken = await saveRefused(page);
    // The marked row is in view under the notice: it has the keyboard.
    await expect(emailBox(page, 0)).toBeFocused();
    const review = await carryOver(page, taken, theirs);

    // Their own dialog, with their own values kept, a blank filled and the
    // lists grown by what they didn't have.
    await expect(editor(page).getByRole('heading', { name: 'Edit person' })).toBeVisible();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(theirs.display_name);
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('Analytical Engines');
    await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('Analyst');
    await expect(emailRows(page)).toHaveCount(2);
    await expect(emailBox(page, 0)).toHaveValue(email);
    await expect(emailBox(page, 1)).toHaveValue(`ada.home.${stamp}@example.com`);
    await expect(phoneRows(page)).toHaveCount(2);
    await expect(editor(page).getByRole('textbox', { name: 'Notes' })).toHaveValue('Prefers email.');

    // The review says what it did, and what was typed and not used: that
    // is how a wrong match is noticed.
    await expect(review).toContainText(`From “${typedName}”, the person you were typing.`);
    await expect(review).toContainText('Nothing is saved until you press Save.');
    await expect(review).toContainText('Added: position, 1 email, 1 phone, notes.');
    await expect(review).toContainText(`You typed the name “${typedName}”; theirs is kept.`);
    await expect(review).toContainText('You typed the company “Engines Ltd”; theirs is kept.');
    // Added rows say so, in words tied to the field; theirs don't. An
    // added row is not an error.
    await expect(emailRows(page).nth(0).getByTestId('carried-mark')).toHaveCount(0);
    await expect(emailRows(page).nth(1).getByTestId('carried-mark')).toHaveText('Added from what you typed');
    await expect(emailBox(page, 1)).toHaveAccessibleDescription('Added from what you typed');
    await expect(emailBox(page, 1)).toHaveAttribute('aria-invalid', 'false');
    await expect(phoneRows(page).nth(1).getByTestId('carried-mark')).toBeVisible();
    await expect(editor(page).getByTestId('carried-mark')).toHaveCount(4);
    // A row changed in review is no longer "added".
    await emailBox(page, 1).fill(`ada.other.${stamp}@example.com`);
    await expect(emailRows(page).nth(1).getByTestId('carried-mark')).toHaveCount(0);
    // So is a field: its mark is about what it holds.
    await expect(editor(page).getByTestId('carried-mark')).toHaveCount(3);
    await editor(page).getByRole('textbox', { name: 'Position' }).fill('Chief Analyst');
    await expect(editor(page).getByTestId('carried-mark')).toHaveCount(2);
    await editor(page).getByRole('textbox', { name: 'Position' }).fill('Analyst');
    await expect(editor(page).getByTestId('carried-mark')).toHaveCount(3);

    // Nothing is saved yet.
    expect((await fetched(request, theirs.id)).emails).toHaveLength(1);

    await editor(page).getByRole('button', { name: 'Save' }).click();
    await expect(editor(page)).toHaveCount(0);
    const after = await fetched(request, theirs.id);
    expect(after.display_name).toBe(theirs.display_name);
    expect(after.company).toBe('Analytical Engines');
    expect(after.position).toBe('Analyst');
    expect(after.emails.map((e) => e.value)).toEqual([email, `ada.other.${stamp}@example.com`]);
    expect(after.emails.map((e) => !!e.preferred)).toEqual([true, false]);
    expect(after.phones.map((p) => p.value)).toEqual(['+1 (555) 010-0000', '+1 555 010 9999']);
    expect(after.notes).toBe('Prefers email.');

    // No second person was made, and the new-person dialog is done with:
    // the registry reopens nothing.
    const all: Person[] = await (await request.get(`/api/people?q=${stamp}`)).json();
    expect(all.map((p) => p.id)).toEqual([theirs.id]);
    await page.goto('/people');
    await expect(page.getByRole('button', { name: 'New person' })).toBeVisible();
    await expect(page.getByTestId('person-row').first()).toBeVisible();
    await expect(editor(page)).toHaveCount(0);
  } finally {
    await wipePeople(request);
  }
});

test('backing out of the review changes nothing and loses nothing', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs, typedName } = await collide(page, request, stamp);
    const review = await carryOver(page, await saveRefused(page), theirs);

    // Escape discards with Undo, and Undo brings the review back whole.
    await page.keyboard.press('Escape');
    await expect(editor(page)).toHaveCount(0);
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(editor(page).getByTestId('carry-review')).toContainText('Added: position');
    await expect(emailRows(page).nth(1).getByTestId('carried-mark')).toBeVisible();

    // A row taken out in review stays out across a reload, and the review
    // is not run a second time.
    await emailRows(page).nth(1).getByRole('button', { name: 'Remove email' }).click();
    await expect(emailRows(page)).toHaveCount(1);
    await page.reload();
    await expect(editor(page).getByTestId('carry-review')).toContainText('Added: position');
    await expect(emailRows(page)).toHaveCount(1);
    await expect(phoneRows(page)).toHaveCount(2);

    // Discard: they are as they were.
    await review.getByRole('button', { name: 'Discard' }).click();
    await expect(editor(page)).toHaveCount(0);
    const after = await fetched(request, theirs.id);
    expect(after).toEqual(theirs);

    // And the new person is still there to go back to, as typed.
    await page.goto('/people');
    await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(typedName);
    await expect(emailRows(page)).toHaveCount(2);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request);
  }
});

test('the review is made against the person as they are now', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs, email } = await collide(page, request, stamp);
    const taken = await saveRefused(page);
    // Changed elsewhere after the refusal and before the click: another
    // number, and the position this form would have filled.
    const changed = await request.put(`/api/people/${theirs.id}`, {
      data: {
        display_name: theirs.display_name,
        company: 'Analytical Engines',
        position: 'Countess',
        emails: [{ label: 'work', value: email, preferred: true }],
        phones: [
          { label: 'mobile', value: '+1 (555) 010-0000', preferred: true },
          { label: 'office', value: '+1 555 010 7777' },
        ],
        addresses: [],
      },
    });
    expect(changed.status()).toBe(200);

    const review = await carryOver(page, taken, theirs);
    // Their new values are there, kept; the typed position is said.
    await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('Countess');
    await expect(review).toContainText('You typed the position “Analyst”; theirs is kept.');
    await expect(phoneRows(page)).toHaveCount(3);
    await editor(page).getByRole('button', { name: 'Save' }).click();
    await expect(editor(page)).toHaveCount(0);
    const after = await fetched(request, theirs.id);
    expect(after.position).toBe('Countess');
    expect(after.phones.map((p) => p.value)).toEqual([
      '+1 (555) 010-0000',
      '+1 555 010 7777',
      '+1 555 010 9999',
    ]);
  } finally {
    await wipePeople(request);
  }
});

test('with two people named, the other one’s address is left out', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const adaMail = `ada.${stamp}@example.com`;
    const beaMail = `bea.${stamp}@example.com`;
    const ada = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: adaMail }] });
    const bea = await person(request, { display_name: `e2e Bea ${stamp}`, emails: [{ value: beaMail }] });
    await newPerson(page, `e2e Both ${stamp}`);
    await addEmail(page, adaMail);
    await addEmail(page, beaMail);
    await addPhone(page, '+1 555 010 4444');
    const taken = await saveRefused(page);
    // Each person once, each with their own button.
    await expect(taken.getByTestId('carry-over')).toHaveCount(2);
    await expect(taken.getByRole('button', { name: bea.display_name, exact: true })).toBeVisible();

    const review = await carryOver(page, taken, ada);
    await expect(review).toContainText('Added: 1 phone.');
    await expect(review).toContainText(`Left out, because it belongs to someone else: ${beaMail}.`);
    await expect(emailRows(page)).toHaveCount(1);
    await editor(page).getByRole('button', { name: 'Save' }).click();
    await expect(editor(page)).toHaveCount(0);
    expect((await fetched(request, ada.id)).emails.map((e) => e.value)).toEqual([adaMail]);
    expect((await fetched(request, bea.id)).emails.map((e) => e.value)).toEqual([beaMail]);
  } finally {
    await wipePeople(request);
  }
});

test('nothing to add: they are opened, and the new person is dropped', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, {
      display_name: `e2e Ada ${stamp}`,
      emails: [{ value: email }],
    });
    // The same name and only the address they have.
    await newPerson(page, theirs.display_name.toUpperCase());
    await addEmail(page, ` ${email}`);
    const taken = await saveRefused(page);
    await carryButton(taken, theirs).click();
    await expect(page).toHaveURL(new RegExp(`/people/${theirs.id}$`));
    await expect(page.getByText('They already have everything you typed.')).toBeVisible();
    await expect(page.getByTestId('person-card')).toBeVisible();
    await expect(editor(page)).toHaveCount(0);
    // Nothing left of the new person to come back to.
    await page.goto('/people');
    await expect(page.getByTestId('person-row').first()).toBeVisible();
    await expect(editor(page)).toHaveCount(0);
  } finally {
    await wipePeople(request);
  }
});

test('a notice older than the form offers nothing stale', async ({ page, request }) => {
  // A short window: the form scrolls, so the notice has to be kept in view.
  await page.setViewportSize({ width: 1024, height: 560 });
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs, typedName } = await collide(page, request, stamp);
    const taken = await saveRefused(page);
    // Only what the API would take as a person is carried: not a form
    // whose name was cleared since.
    const name = editor(page).getByRole('textbox', { name: 'Name' });
    await name.fill('');
    await carryButton(taken, theirs).click();
    await expect(editor(page).getByRole('alert').filter({ hasText: /name is required/i })).toBeVisible();
    await expect(page).toHaveURL(/\/people$/);
    await name.fill(typedName);
    // The address is changed in the form, and the old notice is still up.
    await emailBox(page, 0).fill(`someone.else.${stamp}@example.com`);
    await carryButton(taken, theirs).click();
    await expect(editor(page).getByRole('alert')).toContainText(
      'That address is no longer in this form. Save again.',
    );
    await expect(page).toHaveURL(/\/people$/);
    await expect(editor(page).getByTestId('email-taken')).toHaveCount(0);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request);
  }
});

test('a different new person typed in the meantime is not dropped', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs } = await collide(page, request, stamp);
    await carryOver(page, await saveRefused(page), theirs);
    // Back to the registry, where the new-person dialog returns with its
    // input, and is turned into someone else.
    await page.goBack();
    await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
    const someoneNew = `e2e Someone New ${stamp}`;
    await editor(page).getByRole('textbox', { name: 'Name' }).fill(someoneNew);
    // Forward again: the review is still there, from its own draft.
    await page.goForward();
    await expect(editor(page).getByTestId('carry-review')).toContainText('Added: position');
    await editor(page).getByRole('button', { name: 'Save' }).click();
    await expect(editor(page)).toHaveCount(0);
    // Saving them drops the new-person draft only if it still holds what
    // was carried. It doesn't: it is someone else now, and is kept.
    await page.goto('/people');
    await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(someoneNew);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request);
  }
});

test('an unsaved edit of them that this device kept is said in the review', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, {
      display_name: `e2e Ada ${stamp}`,
      company: 'Analytical Engines',
      emails: [{ value: email }],
    });
    await keepAnEditOf(page, theirs, 'Kept title');
    // And they were changed elsewhere since that edit was kept.
    const changed = await request.put(`/api/people/${theirs.id}`, {
      data: {
        display_name: theirs.display_name,
        company: 'Elsewhere Ltd',
        emails: [{ value: email }],
        phones: [],
        addresses: [],
      },
    });
    expect(changed.status()).toBe(200);

    await newPerson(page, `e2e A. L. ${stamp}`);
    await addEmail(page, email);
    await addPhone(page, '+1 555 010 9999');
    const review = await carryOver(page, await saveRefused(page), theirs);
    // The additions are on top of the kept edit, and the review says so:
    // Save would write that edit too.
    await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('Kept title');
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('Analytical Engines');
    await expect(phoneRows(page)).toHaveCount(1);
    await expect(review).toContainText('Added: 1 phone.');
    await expect(review.getByTestId('carry-kept-edit')).toBeVisible();
    await expect(review).toContainText('The person has been changed since; saving replaces that change.');

    // Both are still said after a reload, and after Escape and Undo.
    await page.reload();
    const again = editor(page).getByTestId('carry-review');
    await expect(again).toContainText('The person has been changed since');
    await expect(again.getByTestId('carry-kept-edit')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(editor(page)).toHaveCount(0);
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(again).toContainText('The person has been changed since');
    await expect(again.getByTestId('carry-kept-edit')).toBeVisible();

    await again.getByRole('button', { name: 'Discard' }).click();
    await expect(editor(page)).toHaveCount(0);
    const after = await fetched(request, theirs.id);
    expect(after.company).toBe('Elsewhere Ltd');
    expect(after.position).toBeNull();
    expect(after.phones).toHaveLength(0);
  } finally {
    await wipePeople(request);
  }
});

test('nothing to add, but an edit of them was kept: their dialog still opens', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
    await keepAnEditOf(page, theirs, 'Kept title');
    await newPerson(page, theirs.display_name);
    await addEmail(page, email);
    const review = await carryOver(page, await saveRefused(page), theirs);
    await expect(review).toContainText('Nothing to add: they already have the rest of what you typed.');
    await expect(review.getByTestId('carry-kept-edit')).toBeVisible();
    await expect(editor(page).getByRole('textbox', { name: 'Position' })).toHaveValue('Kept title');
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request);
  }
});

test('the same button again shows the review as it was left, and more typed joins it', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs } = await collide(page, request, stamp);
    await carryOver(page, await saveRefused(page), theirs);
    // A row taken out in review.
    await emailRows(page).nth(1).getByRole('button', { name: 'Remove email' }).click();
    await expect(emailRows(page)).toHaveCount(1);

    // Back to the new person; the same Save, and the same button.
    await page.goBack();
    await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
    let review = await carryOver(page, await saveRefused(page), theirs);
    // The review as it was left. Not "nothing to add" over additions that
    // are not saved, and not merged a second time.
    await expect(review).toContainText('Added: position, 1 email, 1 phone, notes.');
    await expect(emailRows(page)).toHaveCount(1);
    await expect(phoneRows(page).nth(1).getByTestId('carried-mark')).toBeVisible();

    // More is typed for the new person and carried: one review accounts
    // for both, and the marks of the first are still there.
    await page.goBack();
    await expect(editor(page).getByTestId('form-draft-recovery')).toBeVisible();
    await addPhone(page, '+1 555 010 8888');
    review = await carryOver(page, await saveRefused(page), theirs);
    await expect(review).toContainText('Added: position, 1 email, 2 phones, notes.');
    await expect(phoneRows(page)).toHaveCount(3);
    await expect(phoneRows(page).getByTestId('carried-mark')).toHaveCount(2);
    expect((await fetched(request, theirs.id)).phones).toHaveLength(1);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request);
  }
});

test('only a company typed beyond the address is still something to review', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
    await newPerson(page, theirs.display_name);
    await editor(page).getByRole('textbox', { name: 'Company' }).fill('Engines Ltd');
    await addEmail(page, email);
    const review = await carryOver(page, await saveRefused(page), theirs);
    await expect(review).toContainText('Added: company.');
    await editor(page).getByRole('button', { name: 'Save' }).click();
    await expect(editor(page)).toHaveCount(0);
    expect((await fetched(request, theirs.id)).company).toBe('Engines Ltd');
  } finally {
    await wipePeople(request);
  }
});

test('with the marked row out of view, the keyboard goes to the notice', async ({ page, request }) => {
  await page.setViewportSize({ width: 1024, height: 560 });
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
    await newPerson(page, `e2e Far Down ${stamp}`);
    await addEmail(page, `one.${stamp}@example.com`);
    await addEmail(page, `two.${stamp}@example.com`);
    await addEmail(page, email);
    const taken = await saveRefused(page);
    // The row that is marked is below the fold. Focusing it would scroll
    // the notice away; scrolling back would hide the focus.
    await expect(emailBox(page, 2)).toHaveAttribute('aria-invalid', 'true');
    await expect(emailBox(page, 2)).not.toBeInViewport({ ratio: 1 });
    await expect(taken).toBeFocused();
    // Tab leads on from the notice to what it offers.
    await page.keyboard.press('Tab');
    await expect(taken.getByRole('button', { name: theirs.display_name, exact: true })).toBeFocused();
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request);
  }
});

test('nothing is carried over that this device cannot keep', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  try {
    const { theirs, typedName } = await collide(page, request, stamp);
    const taken = await saveRefused(page);
    // Storage is full or blocked from here on: backing out of a review
    // would have nothing to go back to.
    await page.evaluate(() => {
      Storage.prototype.setItem = () => {
        throw new DOMException('full', 'QuotaExceededError');
      };
    });
    await carryButton(taken, theirs).click();
    await expect(
      editor(page).getByRole('alert').filter({ hasText: 'can’t keep what you typed right now' }),
    ).toBeVisible();
    await expect(page).toHaveURL(/\/people$/);
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(typedName);
    await expect(emailRows(page)).toHaveCount(2);
    // The notice is still there: they can be opened, or the address taken off.
    await expect(taken).toBeVisible();
  } finally {
    await wipePeople(request);
  }
});

// ── On a node ──────────────────────────────────────────────────────────

const PROJECT = 'e2e collide project';

/** On a node's page, "Create “…”" in its People section: the dialog, with `name`. */
async function createOnNode(page: Page, nodeId: string, name: string) {
  await page.goto(`/nodes/${nodeId}`);
  const section = page.getByTestId('node-people');
  await section.getByRole('button', { name: /People/ }).first().click();
  await section.getByRole('button', { name: 'Add person' }).click();
  await createFromPicker(page, name);
  return section;
}

const picker = (page: Page) =>
  page.getByTestId('node-people').getByRole('combobox', { name: 'Find or create a person' });

/** With the picker open: types `name` and takes "Create “…”". */
async function createFromPicker(page: Page, name: string) {
  await picker(page).fill(name);
  await expect(page.getByTestId('node-people').getByRole('option', { selected: true })).toContainText('Create');
  await picker(page).press('Enter');
  await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(name);
}

const onNode = (page: Page) => editor(page).getByRole('group', { name: 'On this node' });

async function typeMembership(page: Page, role: string, note: string) {
  await onNode(page).getByRole('combobox', { name: 'Role' }).fill(role);
  await onNode(page).getByRole('textbox', { name: 'Note' }).fill(note);
}

const formDrafts = (page: Page) =>
  page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith('ember_trove_form_draft:')));

test('on a node, the person who has the address is added there with the role and note typed', async ({
  page,
  request,
}) => {
  const stamp = newStamp();
  await wipePeople(request);
  await deleteNodesByPrefix(request, PROJECT);
  const node = await createNode(request, `${PROJECT} ${stamp}`, { node_type: 'project' });
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, {
      display_name: `e2e Ada ${stamp}`,
      company: 'Analytical Engines',
      emails: [{ value: email }],
    });
    const section = await createOnNode(page, node.id, `e2e A. L. ${stamp}`);
    await editor(page).getByRole('textbox', { name: 'Company' }).fill('Engines Ltd');
    await addEmail(page, email);
    await typeMembership(page, 'Sponsor', 'signs off');
    const taken = await saveRefused(page);
    // What it offers here is the node, not a carry-over; and it says
    // what does not go with them.
    await expect(taken.getByTestId('carry-over')).toHaveCount(0);
    await expect(taken).toContainText('The other details you typed are not added to them and are not kept.');
    await taken.getByRole('button', { name: `Add ${theirs.display_name} here`, exact: true }).click();

    await expect(editor(page)).toHaveCount(0);
    await expect(page.getByText(`${theirs.display_name} added here.`)).toBeVisible();
    await expect(picker(page)).toHaveCount(0);
    const rows = section.getByTestId('node-person');
    await expect(rows).toHaveCount(1);
    await expect(rows.first()).toContainText(theirs.display_name);
    await expect(rows.first().getByTestId('member-role')).toHaveText('Sponsor');
    await expect(rows.first()).toContainText('signs off');
    // No second person, and the other details typed were not put on them.
    const all: Person[] = await (await request.get(`/api/people?q=${stamp}`)).json();
    expect(all.map((p) => p.id)).toEqual([theirs.id]);
    expect(await fetched(request, theirs.id)).toEqual(theirs);
    // The dialog is done with: nothing is left to reopen.
    expect(await formDrafts(page)).toEqual([]);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
    await wipePeople(request);
  }
});

test('on a node, someone already there is said, and a stale notice adds nobody', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  await deleteNodesByPrefix(request, PROJECT);
  const node = await createNode(request, `${PROJECT} ${stamp}`, { node_type: 'project' });
  try {
    const adaMail = `ada.${stamp}@example.com`;
    const beaMail = `bea.${stamp}@example.com`;
    const ada = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: adaMail }] });
    const bea = await person(request, { display_name: `e2e Bea ${stamp}`, emails: [{ value: beaMail }] });
    const there = await request.post(`/api/nodes/${node.id}/people`, { data: { person_id: ada.id } });
    expect(there.status(), await there.text()).toBe(201);

    const section = await createOnNode(page, node.id, `e2e Both ${stamp}`);
    await addEmail(page, adaMail);
    await addEmail(page, beaMail);
    await typeMembership(page, 'Reviewer', 'second pair of eyes');
    const taken = await saveRefused(page);
    // Ada is on the node: said, with nothing to press. Bea can be added.
    const rows = taken.getByRole('listitem');
    await expect(rows.filter({ hasText: ada.display_name }).getByTestId('already-here')).toHaveText('Already here.');
    await expect(rows.filter({ hasText: ada.display_name }).getByTestId('add-here')).toHaveCount(0);
    const addBea = taken.getByRole('button', { name: `Add ${bea.display_name} here`, exact: true });
    await expect(addBea).toBeVisible();

    // Bea's address is taken off the form, and the old notice is still up.
    await emailBox(page, 1).fill(`someone.else.${stamp}@example.com`);
    await addBea.click();
    const stale = editor(page).getByRole('alert');
    await expect(stale).toContainText('That address is no longer in this form. Save again.');
    await expect(editor(page).getByTestId('email-taken')).toHaveCount(0);
    // The pressed button went with the notice: the keyboard is on what
    // says why, not on the page behind the dialog.
    await expect(stale).toBeFocused();
    // Nobody was added, and what was typed is all still there.
    await expect(onNode(page).getByRole('combobox', { name: 'Role' })).toHaveValue('Reviewer');
    const members: unknown[] = await (await request.get(`/api/nodes/${node.id}/people`)).json();
    expect(members).toHaveLength(1);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
    await expect(section.getByTestId('node-person')).toHaveCount(1);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
    await wipePeople(request);
  }
});

test('on a node, an add that fails loses nothing, and a second press works', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  await deleteNodesByPrefix(request, PROJECT);
  const node = await createNode(request, `${PROJECT} ${stamp}`, { node_type: 'project' });
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
    const section = await createOnNode(page, node.id, `e2e A. L. ${stamp}`);
    await addEmail(page, email);
    await typeMembership(page, 'Sponsor', 'signs off');
    const taken = await saveRefused(page);

    // The first add is held, then fails; later ones go through.
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    let posts = 0;
    await page.route(`**/api/nodes/${node.id}/people`, async (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      posts += 1;
      if (posts > 1) return route.fallback();
      await held;
      return route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":"boom"}' });
    });
    const add = taken.getByRole('button', { name: `Add ${theirs.display_name} here`, exact: true });
    await add.click();
    // While it is on its way the button can't be pressed again, and the
    // dialog stays: it closes only once the add has succeeded.
    await expect(add).toBeDisabled();
    await expect(editor(page)).toBeVisible();
    // A press that gets through anyway (the button keeps the keyboard, so
    // it is not `disabled`) sends nothing.
    await add.dispatchEvent('click');
    release();
    await expect(taken.getByTestId('add-here-failed')).toContainText('Nothing you typed is lost');
    expect(posts).toBe(1);
    await expect(add).toBeFocused();
    await expect(onNode(page).getByRole('combobox', { name: 'Role' })).toHaveValue('Sponsor');
    await expect(onNode(page).getByRole('textbox', { name: 'Note' })).toHaveValue('signs off');
    await expect(section.getByTestId('node-person')).toHaveCount(0);

    await expect(add).toBeEnabled();
    await add.click();
    await expect(editor(page)).toHaveCount(0);
    const rows = section.getByTestId('node-person');
    await expect(rows).toHaveCount(1);
    await expect(rows.first().getByTestId('member-role')).toHaveText('Sponsor');
    expect(posts).toBe(2);
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await request.delete(`/api/nodes/${node.id}`);
    await wipePeople(request);
  }
});

test('on a node, someone put there a moment ago is said, not added twice', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  await deleteNodesByPrefix(request, PROJECT);
  const node = await createNode(request, `${PROJECT} ${stamp}`, { node_type: 'project' });
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
    await createOnNode(page, node.id, `e2e A. L. ${stamp}`);
    await addEmail(page, email);
    await typeMembership(page, 'Sponsor', 'signs off');
    const taken = await saveRefused(page);
    const add = taken.getByRole('button', { name: `Add ${theirs.display_name} here`, exact: true });
    await expect(add).toBeVisible();
    // Another tab puts them on the node after the notice was drawn.
    const there = await request.post(`/api/nodes/${node.id}/people`, {
      data: { person_id: theirs.id, role: 'Approver' },
    });
    expect(there.status()).toBe(201);
    await add.click();
    await expect(taken.getByTestId('already-here')).toHaveText('Already here.');
    await expect(taken.getByTestId('add-here')).toHaveCount(0);
    await expect(taken.getByTestId('add-here-failed')).toHaveCount(0);
    // The button is gone; the keyboard is on the notice.
    await expect(taken).toBeFocused();
    // The dialog is kept, with what was typed; their role there is the
    // one they were given, not overwritten.
    await expect(onNode(page).getByRole('combobox', { name: 'Role' })).toHaveValue('Sponsor');
    const members: { role: string | null }[] = await (await request.get(`/api/nodes/${node.id}/people`)).json();
    expect(members.map((m) => m.role)).toEqual(['Approver']);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
    await wipePeople(request);
  }
});

test('on a node, an answer that comes late leaves a later dialog alone', async ({ page, request }) => {
  const stamp = newStamp();
  await wipePeople(request);
  await deleteNodesByPrefix(request, PROJECT);
  const node = await createNode(request, `${PROJECT} ${stamp}`, { node_type: 'project' });
  try {
    const email = `ada.${stamp}@example.com`;
    const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
    // The first add fails at once; the second is held, then goes through.
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    let posts = 0;
    await page.route(`**/api/nodes/${node.id}/people`, async (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      posts += 1;
      if (posts === 1) {
        return route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":"boom"}' });
      }
      if (posts === 2) await held;
      return route.fallback();
    });
    const addButton = () =>
      editor(page).getByRole('button', { name: `Add ${theirs.display_name} here`, exact: true });
    const cancel = async () => {
      await editor(page).getByRole('button', { name: 'Cancel' }).click();
      await expect(editor(page)).toHaveCount(0);
    };

    const section = await createOnNode(page, node.id, `e2e A. L. ${stamp}`);
    await addEmail(page, email);
    let taken = await saveRefused(page);
    await addButton().click();
    await expect(taken.getByTestId('add-here-failed')).toBeVisible();

    // Cancel and the same again: a new opening does not show the old failure.
    await cancel();
    await createFromPicker(page, `e2e A. L. ${stamp}`);
    await addEmail(page, email);
    taken = await saveRefused(page);
    await expect(taken.getByTestId('add-here-failed')).toHaveCount(0);

    // Pressed, and while it is on its way the dialog is cancelled and
    // another person is being typed.
    await addButton().click();
    await expect(addButton()).toBeDisabled();
    await cancel();
    const later = `e2e Someone Later ${stamp}`;
    await createFromPicker(page, later);
    await editor(page).getByRole('textbox', { name: 'Company' }).fill('Typed meanwhile');
    release();

    // The add went through and is said; the dialog now open is not the
    // one it was pressed in, and keeps what it holds.
    await expect(page.getByText(`${theirs.display_name} added here.`)).toBeVisible();
    await expect(section.getByTestId('node-person')).toHaveCount(1);
    await expect(editor(page).getByRole('textbox', { name: 'Name' })).toHaveValue(later);
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toHaveValue('Typed meanwhile');
    // The keyboard too: the list read again behind it does not take it.
    await expect(editor(page).getByRole('textbox', { name: 'Company' })).toBeFocused();
    expect(posts).toBe(2);
    await cancel();
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await request.delete(`/api/nodes/${node.id}`);
    await wipePeople(request);
  }
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the notice and the review fit', async ({ page, request }) => {
    const stamp = newStamp();
    await wipePeople(request);
    try {
      const { theirs } = await collide(page, request, stamp);
      const taken = await saveRefused(page);
      const fits = () =>
        editor(page)
          .getByTestId('person-form')
          .evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(await fits()).toBeLessThanOrEqual(1);
      await carryOver(page, taken, theirs);
      expect(await fits()).toBeLessThanOrEqual(1);
      const save = editor(page).getByRole('button', { name: 'Save' });
      const box = await save.boundingBox();
      expect(box && box.x + box.width).toBeLessThanOrEqual(375);
      await editor(page).getByRole('button', { name: 'Cancel' }).click();
    } finally {
      await wipePeople(request);
    }
  });

  test('the notice on a node fits', async ({ page, request }) => {
    const stamp = newStamp();
    await wipePeople(request);
    await deleteNodesByPrefix(request, PROJECT);
    const node = await createNode(request, `${PROJECT} ${stamp}`, { node_type: 'project' });
    try {
      const email = `ada.${stamp}@example.com`;
      const theirs = await person(request, { display_name: `e2e Ada ${stamp}`, emails: [{ value: email }] });
      await createOnNode(page, node.id, `e2e A. L. ${stamp}`);
      await addEmail(page, email);
      const taken = await saveRefused(page);
      const form = editor(page).getByTestId('person-form');
      expect(await form.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(1);
      const box = await taken
        .getByRole('button', { name: `Add ${theirs.display_name} here`, exact: true })
        .boundingBox();
      expect(box && box.x + box.width).toBeLessThanOrEqual(375);
      await editor(page).getByRole('button', { name: 'Cancel' }).click();
    } finally {
      await request.delete(`/api/nodes/${node.id}`);
      await wipePeople(request);
    }
  });
});
