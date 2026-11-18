import { test, expect, type APIRequestContext, type Page } from './fixtures';

/**
 * What the app offers when an email address already belongs to someone
 * (S8b, #357; ONTOLOGY §7.1: "open the existing record or merge into
 * it"). A new person who collides can be added to the existing one for
 * review: their own values are kept, blanks are filled, lists grow, and
 * nothing is saved until Save. Backing out loses nothing.
 *
 * People are per account, so one identity is enough. Every person's name
 * holds the test's stamp; `wipePeople` removes them before and after.
 */

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

async function wipePeople(request: APIRequestContext, stamp: string) {
  const people: Person[] = await (await request.get(`/api/people?q=${encodeURIComponent(stamp)}`)).json();
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
  // The keyboard goes to the marked row, and the notice that says why
  // stays in view: focusing the row used to scroll it away (3.30.0).
  await expect(emailRows(page).locator('input[aria-invalid="true"]').first()).toBeFocused();
  await expect(taken).toBeInViewport({ ratio: 1 });
  return taken;
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
  await taken.getByRole('button', { name: `Add what I typed to ${theirs.display_name}` }).click();
  await expect(page).toHaveURL(new RegExp(`/people/${theirs.id}$`));
  const review = editor(page).getByTestId('carry-review');
  await expect(review).toBeVisible();
  return review;
}

test('what was typed is added to the person who has the address, reviewed, then saved', async ({
  page,
  request,
}) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
  try {
    const { theirs, email, typedName } = await collide(page, request, stamp);
    const taken = await saveRefused(page);
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
    await wipePeople(request, stamp);
  }
});

test('backing out of the review changes nothing and loses nothing', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
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
    await wipePeople(request, stamp);
  }
});

test('the review is made against the person as they are now', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
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
    await wipePeople(request, stamp);
  }
});

test('with two people named, the other one’s address is left out', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
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
    await wipePeople(request, stamp);
  }
});

test('nothing to add: they are opened, and the new person is dropped', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
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
    await taken.getByRole('button', { name: `Add what I typed to ${theirs.display_name}` }).click();
    await expect(page).toHaveURL(new RegExp(`/people/${theirs.id}$`));
    await expect(page.getByText('They already have everything you typed.')).toBeVisible();
    await expect(page.getByTestId('person-card')).toBeVisible();
    await expect(editor(page)).toHaveCount(0);
    // Nothing left of the new person to come back to.
    await page.goto('/people');
    await expect(page.getByTestId('person-row').first()).toBeVisible();
    await expect(editor(page)).toHaveCount(0);
  } finally {
    await wipePeople(request, stamp);
  }
});

test('a notice older than the form offers nothing stale', async ({ page, request }) => {
  // A short window: the form scrolls, so the notice has to be kept in view.
  await page.setViewportSize({ width: 1024, height: 560 });
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
  try {
    const { theirs } = await collide(page, request, stamp);
    const taken = await saveRefused(page);
    // The address is changed in the form, and the old notice is still up.
    await emailBox(page, 0).fill(`someone.else.${stamp}@example.com`);
    await taken.getByRole('button', { name: `Add what I typed to ${theirs.display_name}` }).click();
    await expect(editor(page).getByRole('alert')).toContainText(
      'That address is no longer in this form. Save again.',
    );
    await expect(page).toHaveURL(/\/people$/);
    await expect(editor(page).getByTestId('email-taken')).toHaveCount(0);
    await editor(page).getByRole('button', { name: 'Cancel' }).click();
  } finally {
    await wipePeople(request, stamp);
  }
});

test('a different new person typed in the meantime is not dropped', async ({ page, request }) => {
  const stamp = `${Date.now()}`;
  await wipePeople(request, stamp);
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
    await wipePeople(request, stamp);
  }
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('the notice and the review fit', async ({ page, request }) => {
    const stamp = `${Date.now()}`;
    await wipePeople(request, stamp);
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
      await wipePeople(request, stamp);
    }
  });
});
