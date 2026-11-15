import { test, expect, createNode, shareNode } from './fixtures';
import type { APIRequestContext, Page } from '@playwright/test';

/**
 * History (S6d, #329; ONTOLOGY §9.2, §10): what occurred about a node, in
 * one chronology. The first tests hold the rules through the API: a note's
 * "edited" mark, what a reader may do with a note, and the split of a
 * node's log into its History and its audit trail. The rest are the
 * History section of a node's page, which was "Notes".
 */

const OTHER_SUBJECT = 'e2e-other-user';
const PREFIX = 'e2e history ';

type Note = {
  id: string;
  body: string;
  color: string;
  created_at: string;
  edited_at: string | null;
  may_edit?: boolean;
  may_remove?: boolean;
};

async function addNote(request: APIRequestContext, nodeId: string, body: string): Promise<Note> {
  const resp = await request.post(`/api/nodes/${nodeId}/notes`, { data: { body, color: 'default' } });
  expect(resp.ok(), `note ${body}`).toBeTruthy();
  return resp.json();
}

const notesOf = async (request: APIRequestContext, nodeId: string): Promise<Note[]> =>
  (await request.get(`/api/nodes/${nodeId}/notes`)).json();

const edit = (request: APIRequestContext, id: string, body: string, color = 'default') =>
  request.patch(`/api/notes/${id}`, { data: { body, color } });

type Entry = { action: string; metadata: Record<string, unknown> };
const log = async (request: APIRequestContext, nodeId: string, scope?: string): Promise<Entry[]> =>
  (await request.get(`/api/nodes/${nodeId}/activity${scope ? `?scope=${scope}` : ''}`)).json();

async function wipe(request: APIRequestContext) {
  for (let round = 0; round < 5; round++) {
    const hits: { id: string; title: string }[] = await (
      await request.get(`/api/nodes/lookup?q=${encodeURIComponent(PREFIX.trim())}&limit=20`)
    ).json();
    const mine = hits.filter((n) => n.title.startsWith(PREFIX));
    if (mine.length === 0) break;
    for (const n of mine) await request.delete(`/api/nodes/${n.id}`);
  }
}

test.beforeEach(async ({ request, otherRequest }) => {
  await wipe(otherRequest);
  await wipe(request);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(otherRequest);
  await wipe(request);
});

test('a note is "edited" when its words change, and its date never moves', async ({ request }) => {
  const node = await createNode(request, `${PREFIX}marks ${Date.now()}`);
  const note = await addNote(request, node.id, 'first words');
  expect(note.edited_at).toBeNull();

  // Not edits: a colour, the same words again, a delete and its Undo.
  let saved: Note = await (await edit(request, note.id, 'first words', 'amber')).json();
  expect(saved.color).toBe('amber');
  expect(saved.edited_at).toBeNull();
  saved = await (await edit(request, note.id, 'first words', 'amber')).json();
  expect(saved.edited_at).toBeNull();
  expect((await request.delete(`/api/notes/${note.id}`)).ok()).toBeTruthy();
  saved = await (await request.post(`/api/notes/${note.id}/restore`)).json();
  expect(saved.edited_at).toBeNull();

  // An edit: marked, with a date; the note stays where it was written.
  saved = await (await edit(request, note.id, 'second words', 'amber')).json();
  expect(saved.edited_at).not.toBeNull();
  expect(saved.created_at).toBe(note.created_at);
  const firstEdit = saved.edited_at;
  // Recolouring an edited note leaves the mark's date alone.
  saved = await (await edit(request, note.id, 'second words', 'rose')).json();
  expect(saved.edited_at).toBe(firstEdit);

  const listed = (await notesOf(request, node.id)).find((n) => n.id === note.id);
  expect(listed?.edited_at).toBe(firstEdit);
  expect(listed?.created_at).toBe(note.created_at);
});

test('a note says what its reader may do with it, and the API agrees', async ({
  request,
  otherRequest,
}) => {
  const node = await createNode(request, `${PREFIX}rights ${Date.now()}`);
  await shareNode(request, node.id, OTHER_SUBJECT, 'editor');
  const theirs = await addNote(otherRequest, node.id, 'written by the other person');
  const mine = await addNote(request, node.id, 'written by the owner');
  const flags = async (as: APIRequestContext, id: string) => {
    const note = (await notesOf(as, node.id)).find((n) => n.id === id);
    return { edit: note?.may_edit, remove: note?.may_remove };
  };

  // Its author, while an editor: both. The node's owner: remove only.
  expect(await flags(otherRequest, theirs.id)).toEqual({ edit: true, remove: true });
  expect(await flags(request, theirs.id)).toEqual({ edit: false, remove: true });
  // The owner's own note: the other editor may do neither.
  expect(await flags(request, mine.id)).toEqual({ edit: true, remove: true });
  expect(await flags(otherRequest, mine.id)).toEqual({ edit: false, remove: false });

  // Each flag is what the write does. A refusal reads as "not found".
  expect((await edit(request, theirs.id, 'the owner puts words in it')).status()).toBe(404);
  expect((await edit(otherRequest, mine.id, 'and the reverse')).status()).toBe(404);
  expect((await otherRequest.delete(`/api/notes/${mine.id}`)).status()).toBe(404);
  expect((await edit(otherRequest, theirs.id, 'its author edits it')).ok()).toBeTruthy();

  // Demoted to viewer, the author keeps reading and loses both.
  await shareNode(request, node.id, OTHER_SUBJECT, 'viewer');
  expect(await flags(otherRequest, theirs.id)).toEqual({ edit: false, remove: false });
  expect((await edit(otherRequest, theirs.id, 'as a viewer')).status()).toBe(404);
  // The owner removes what was left behind.
  expect((await request.delete(`/api/notes/${theirs.id}`)).ok()).toBeTruthy();
});

test('a node\'s History is read apart from its audit trail, and publishing is recorded', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  // An Article: History is on every kind.
  const article = await createNode(request, `${PREFIX}article ${stamp}`);
  const actions = (entries: Entry[]) => entries.map((e) => e.action).sort();

  // Published, with nothing else in the save (the app's editor sends the
  // text too, and that is an edit): History gets a line, and the audit
  // trail gets no "edited" beside it.
  const published = await request.put(`/api/nodes/${article.id}`, { data: { status: 'published' } });
  expect(published.ok()).toBeTruthy();
  let history = await log(request, article.id, 'history');
  expect(actions(history)).toEqual(['lifecycle_changed']);
  expect(history[0].metadata.from).toBe('draft');
  expect(history[0].metadata.to).toBe('published');
  expect(actions(await log(request, article.id, 'audit'))).toEqual(['created']);

  // The same state again is no change. Archiving is one.
  await request.put(`/api/nodes/${article.id}`, { data: { status: 'published' } });
  await request.put(`/api/nodes/${article.id}`, { data: { status: 'archived' } });
  history = await log(request, article.id, 'history');
  expect(history.map((e) => `${e.metadata.from} > ${e.metadata.to}`)).toEqual([
    'published > archived',
    'draft > published',
  ]);
  // An edit of the text is audit, not History.
  await request.put(`/api/nodes/${article.id}`, { data: { body: 'some words' } });
  expect(actions(await log(request, article.id, 'audit'))).toEqual(['created', 'edited']);
  expect(await log(request, article.id, 'history')).toHaveLength(2);
  // Without a scope: everything, as an older page asks.
  expect(await log(request, article.id)).toHaveLength(4);

  // A Project's status label is History too.
  const project = await createNode(request, `${PREFIX}project ${stamp}`, { node_type: 'project' });
  await request.put(`/api/nodes/${project.id}`, { data: { project_status: 'Blocked' } });
  expect(actions(await log(request, project.id, 'history'))).toEqual(['status_changed']);

  // Rights before the scope: a stranger learns nothing from it.
  for (const scope of ['history', 'audit', 'nonsense']) {
    const resp = await otherRequest.get(`/api/nodes/${article.id}/activity?scope=${scope}`);
    expect(resp.status(), scope).toBe(403);
  }
  expect((await request.get(`/api/nodes/${article.id}/activity?scope=nonsense`)).status()).toBe(422);
});

// ── The History section ─────────────────────────────────────────────────────

const history = (page: Page) => page.getByTestId('node-history');
const card = (page: Page, id: string) => page.locator(`[data-note-id="${id}"]`);

/** What the list holds, top to bottom: 'note' or the action of a change. */
const order = (page: Page): Promise<string[]> =>
  page
    .getByTestId('history-list')
    .locator('> *')
    .evaluateAll((els) =>
      els.map((el) =>
        (el as HTMLElement).dataset.testid === 'history-note'
          ? 'note'
          : ((el as HTMLElement).dataset.action ?? '?'),
      ),
    );

/** Declare a state with the project's state chip, and wait for the save. */
async function declareInPage(page: Page, state: string) {
  const chip = page.getByTestId('project-state-chip');
  await expect(chip).toHaveAttribute('aria-disabled', 'false');
  await chip.click();
  await page.getByTestId(`project-state-option-${state}`).click();
  await expect(chip).toHaveAttribute('data-state', state);
  await expect(chip).toHaveAttribute('aria-disabled', 'false');
}

test('History holds notes and changes in one order, and Activity keeps the audit trail', async ({
  page,
  request,
}) => {
  const project = await createNode(request, `${PREFIX}chronology ${Date.now()}`, {
    node_type: 'project',
  });
  const first = await addNote(request, project.id, 'the first note');
  await request.put(`/api/nodes/${project.id}`, { data: { project_status: 'Blocked' } });
  const second = await addNote(request, project.id, 'the second note');

  await page.goto(`/nodes/${project.id}`);
  await expect(card(page, second.id)).toBeVisible();
  // Newest first: the change sits between the two notes, where it happened.
  await expect.poll(() => order(page)).toEqual(['note', 'status_changed', 'note']);
  await expect(history(page).locator('[data-note-id]').first()).toHaveAttribute(
    'data-note-id',
    second.id,
  );
  await expect(history(page).getByText(/changed status/)).toHaveCount(1);

  // A change made on the page shows at the top, without a reload.
  await declareInPage(page, 'parked');
  await expect.poll(() => order(page)).toEqual([
    'project_state_changed',
    'note',
    'status_changed',
    'note',
  ]);
  await expect(history(page).getByText('Active → Parked')).toHaveCount(1);
  await expect(card(page, first.id)).toBeVisible();

  // Activity is who touched the node, and nothing History shows.
  const activity = page.getByTestId('node-activity');
  const toggle = activity.locator('button[aria-expanded]').first();
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
  await expect(activity.getByText(/created/)).toHaveCount(1);
  await expect(activity.getByText(/changed status|changed state/)).toHaveCount(0);
});

test('a note edited on the page says so and stays where it was written', async ({ page, request }) => {
  const node = await createNode(request, `${PREFIX}edit ${Date.now()}`);
  const older = await addNote(request, node.id, 'the older note');
  const newer = await addNote(request, node.id, 'the newer note');
  await page.goto(`/nodes/${node.id}`);
  const editedMark = (id: string) => card(page, id).getByTestId('note-edited');
  await expect(card(page, older.id)).toBeVisible();
  await expect(editedMark(older.id)).toHaveCount(0);

  // The older note is edited: marked, and still under the newer one.
  await card(page, older.id).locator('button[title="Edit note"]').click();
  await page.getByPlaceholder('Note…').fill('the older note, with a correction');
  await card(page, older.id).getByRole('button', { name: 'Save', exact: true }).click();
  await expect(card(page, older.id)).toContainText('with a correction');
  await expect(editedMark(older.id)).toBeVisible();
  await expect(editedMark(older.id)).toHaveAttribute('title', /^Edited /);
  await expect(history(page).locator('[data-note-id]').first()).toHaveAttribute(
    'data-note-id',
    newer.id,
  );

  // Not edits: a new colour, saving without a change, a delete and Undo.
  await card(page, newer.id).locator('button[title="Edit note"]').click();
  await card(page, newer.id).locator('button[title="amber"]').click();
  await card(page, newer.id).getByRole('button', { name: 'Save', exact: true }).click();
  await expect(card(page, newer.id).getByPlaceholder('Note…')).toHaveCount(0);
  // The card is drawn again from what was saved, at once: an Edit opened
  // now starts from the amber note, not from the one before it.
  await expect(card(page, newer.id)).toHaveClass(/bg-amber-100/);
  await expect(editedMark(newer.id)).toHaveCount(0);
  // Opened and saved as it is: nothing is sent at all.
  let patches = 0;
  page.on('request', (r) => {
    if (r.method() === 'PATCH' && r.url().includes('/api/notes/')) patches += 1;
  });
  await card(page, newer.id).locator('button[title="Edit note"]').click();
  await card(page, newer.id).getByRole('button', { name: 'Save', exact: true }).click();
  await expect(card(page, newer.id).getByPlaceholder('Note…')).toHaveCount(0);
  expect(patches).toBe(0);
  await card(page, newer.id).getByRole('button', { name: 'Delete note' }).click();
  await expect(card(page, newer.id)).toHaveCount(0);
  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(card(page, newer.id)).toBeVisible();
  await expect(editedMark(newer.id)).toHaveCount(0);
  const listed = (await notesOf(request, node.id)).find((n) => n.id === newer.id);
  expect(listed?.edited_at).toBeNull();
  expect(listed?.color).toBe('amber');
});

test('a note being edited is left alone when History reads again', async ({ page, request }) => {
  const project = await createNode(request, `${PREFIX}open edit ${Date.now()}`, {
    node_type: 'project',
  });
  const note = await addNote(request, project.id, 'a note about to be edited');
  await page.goto(`/nodes/${project.id}`);
  await card(page, note.id).locator('button[title="Edit note"]').click();
  const editor = card(page, note.id).getByPlaceholder('Note…');
  await editor.fill('typed, and not saved yet');

  // The project is parked from its header: History reads its changes
  // again and gains a line. The open editor keeps what was typed.
  await declareInPage(page, 'parked');
  await expect(history(page).getByText('Active → Parked')).toHaveCount(1);
  await expect(editor).toHaveValue('typed, and not saved yet');
  // Another note is added from the box: the notes are read again, and
  // the new card arrives above. The editor still holds the text.
  await page.locator('button[title="Add note"]').click();
  await page.getByPlaceholder('Write a note… (Ctrl+Enter to save)').fill('added meanwhile');
  await page.locator('button[title="Save note"]').click();
  await expect(history(page).getByText('added meanwhile')).toBeVisible();
  await expect(editor).toHaveValue('typed, and not saved yet');
  // And another note is deleted and brought back with Undo.
  const other = history(page).locator('[data-note-id]').filter({ hasText: 'added meanwhile' });
  await other.getByRole('button', { name: 'Delete note' }).click();
  await expect(other).toHaveCount(0);
  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(other).toBeVisible();
  await expect(editor).toHaveValue('typed, and not saved yet');
  await card(page, note.id).getByRole('button', { name: 'Save', exact: true }).click();
  await expect(card(page, note.id)).toContainText('typed, and not saved yet');
});

test('Edit is offered to a note\'s author and Delete to those who may remove it', async ({
  page,
  request,
  otherRequest,
}) => {
  const node = await createNode(request, `${PREFIX}offers ${Date.now()}`);
  await shareNode(request, node.id, OTHER_SUBJECT, 'editor');
  const theirs = await addNote(otherRequest, node.id, 'written by the other person');
  const mine = await addNote(request, node.id, 'written by the owner');
  const edit = (id: string) => card(page, id).locator('button[title="Edit note"]');
  const remove = (id: string) => card(page, id).getByRole('button', { name: 'Delete note' });

  // The node's owner: their own note fully, the other's to delete only.
  // (The page used to offer Edit here, and the API answered "not found".)
  await page.goto(`/nodes/${node.id}`);
  await expect(card(page, theirs.id)).toBeVisible();
  await expect(edit(mine.id)).toBeVisible();
  await expect(remove(mine.id)).toBeVisible();
  await expect(edit(theirs.id)).toHaveCount(0);
  await expect(remove(theirs.id)).toBeVisible();

  // The other editor: their own note fully, the owner's not at all.
  await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
  try {
    await page.goto(`/nodes/${node.id}`);
    await expect(card(page, theirs.id)).toBeVisible();
    await expect(edit(theirs.id)).toBeVisible();
    await expect(remove(theirs.id)).toBeVisible();
    await expect(edit(mine.id)).toHaveCount(0);
    await expect(remove(mine.id)).toHaveCount(0);
    // And the offer is real: their edit saves.
    await edit(theirs.id).click();
    await card(page, theirs.id).getByPlaceholder('Note…').fill('edited by its author');
    await card(page, theirs.id).getByRole('button', { name: 'Save', exact: true }).click();
    await expect(card(page, theirs.id)).toContainText('edited by its author');
    await expect(card(page, theirs.id).getByTestId('note-edited')).toBeVisible();
  } finally {
    await page.setExtraHTTPHeaders({});
  }
});

test('the notes still show when the changes fail to load', async ({ page, request }) => {
  const project = await createNode(request, `${PREFIX}half ${Date.now()}`, { node_type: 'project' });
  const note = await addNote(request, project.id, 'a note that must still show');
  await request.put(`/api/nodes/${project.id}`, { data: { project_status: 'Blocked' } });
  const failing = `**/api/nodes/${project.id}/activity?scope=history**`;
  await page.route(failing, (route) => route.fulfill({ status: 500, body: 'no' }));
  await page.goto(`/nodes/${project.id}`);
  await expect(card(page, note.id)).toBeVisible();
  // Said, not hidden: the list is the notes alone, and it says so.
  const failed = page.getByTestId('history-changes-error');
  await expect(failed).toContainText('Couldn’t load the changes');
  await expect(history(page).getByTestId('history-change')).toHaveCount(0);
  await expect(history(page).getByTestId('history-empty')).toHaveCount(0);

  await page.unroute(failing);
  await failed.getByRole('button', { name: 'Try again' }).click();
  await expect(failed).toHaveCount(0);
  await expect(history(page).getByText('Active → Blocked')).toHaveCount(1);
  await expect(card(page, note.id)).toBeVisible();

  // A project with changes and no note: when the changes can't be read,
  // the section does not claim there is nothing.
  const bare = await createNode(request, `${PREFIX}half bare ${Date.now()}`, { node_type: 'project' });
  await request.put(`/api/nodes/${bare.id}`, { data: { project_status: 'Blocked' } });
  const failingBare = `**/api/nodes/${bare.id}/activity?scope=history**`;
  await page.route(failingBare, (route) => route.fulfill({ status: 500, body: 'no' }));
  await page.goto(`/nodes/${bare.id}`);
  await expect(page.getByTestId('history-changes-error')).toBeVisible();
  await expect(history(page).getByTestId('history-empty')).toHaveCount(0);
  await page.unroute(failingBare);
  await page.getByTestId('history-changes-error').getByRole('button', { name: 'Try again' }).click();
  await expect(history(page).getByText('Active → Blocked')).toHaveCount(1);
  await expect(history(page).getByTestId('history-empty')).toHaveCount(0);
});

test('a cascade\'s line opens to what it closed, and a publish shows on an Article', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const master = await createNode(request, `${PREFIX}cascade ${stamp}`, { node_type: 'project' });
  const child = await createNode(request, `${PREFIX}cascade child ${stamp}`, { node_type: 'project' });
  expect(
    (
      await request.post('/api/edges', {
        data: { source_id: master.id, target_id: child.id, edge_type: 'contains' },
      })
    ).ok(),
  ).toBeTruthy();
  const task = await (
    await request.post(`/api/nodes/${master.id}/tasks`, { data: { title: `${PREFIX}task ${stamp}` } })
  ).json();
  const plan = await (
    await request.get(`/api/nodes/${master.id}/project-state/cascade?state=complete`)
  ).json();
  expect(
    (
      await request.put(`/api/nodes/${master.id}/project-state`, {
        data: { state: 'complete', confirm: plan.token },
      })
    ).ok(),
  ).toBeTruthy();

  await page.goto(`/nodes/${master.id}`);
  const line = history(page).locator('[data-action="cascade_closed"]');
  await expect(line).toContainText('closed what was under it');
  await expect(line).toContainText('1 sub-project, 1 task');
  // Closed, it is one line. Opened, it names exactly what it closed.
  await expect(line.getByTestId('history-change-details')).toHaveCount(0);
  await line.getByTestId('history-change-toggle').click();
  const details = line.getByTestId('history-change-details');
  await expect(details).toContainText(child.title);
  await expect(details).toContainText(task.title);
  await line.getByTestId('history-change-toggle').click();
  await expect(details).toHaveCount(0);
  // A plain change is there, with nothing more to open.
  const stateLine = history(page).locator('[data-action="project_state_changed"]');
  await expect(stateLine).toContainText('Active → Complete');
  await expect(stateLine.getByTestId('history-change-toggle')).toHaveCount(0);

  // History is on every kind: an Article's publishing is in its own.
  const article = await createNode(request, `${PREFIX}article ${stamp}`);
  await request.put(`/api/nodes/${article.id}`, { data: { status: 'published' } });
  await page.goto(`/nodes/${article.id}`);
  await expect(history(page).getByText('Draft → Published')).toHaveCount(1);
  await expect(history(page).getByText(/changed stage/)).toHaveCount(1);
});

test('a replaced Brief shows in History at once, and opens to its old words', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const project = await createNode(request, `${PREFIX}brief ${stamp}`, { node_type: 'project' });
  // Someone else stated the Brief, so the owner's save replaces it (a
  // save by its own author within a few minutes corrects it in place,
  // and leaves no line).
  await shareNode(request, project.id, OTHER_SUBJECT, 'editor');
  const stated = await otherRequest.put(`/api/nodes/${project.id}/brief`, {
    data: { headline: `Old headline ${stamp}`, pitch: 'The old pitch.', points: ['An old point'] },
  });
  expect(stated.ok()).toBeTruthy();

  await page.goto(`/nodes/${project.id}`);
  const brief = page.getByTestId('node-brief');
  await expect(brief.getByTestId('brief-headline')).toHaveText(`Old headline ${stamp}`);
  await expect(history(page).locator('[data-action="brief_superseded"]')).toHaveCount(0);
  await brief.getByRole('button', { name: 'Edit brief' }).click();
  await brief.getByTestId('brief-editor').getByLabel('Headline').fill(`New headline ${stamp}`);
  await brief.getByTestId('brief-editor').getByRole('button', { name: 'Save' }).click();
  await expect(brief.getByTestId('brief-headline')).toHaveText(`New headline ${stamp}`);

  // History gained the line without a reload, worded as an update.
  const line = history(page).locator('[data-action="brief_superseded"]');
  await expect(line).toContainText('updated the brief');
  await expect(line).toContainText(`was: Old headline ${stamp}`);
  await line.getByTestId('history-change-toggle').click();
  const details = line.getByTestId('history-change-details');
  await expect(details).toContainText('The brief before this');
  await expect(details).toContainText('The old pitch.');
  // One bullet per point: the list draws it.
  await expect(details.locator('li')).toHaveText(['An old point']);
});

test('an empty History says what will show there, and Show more counts every line', async ({
  page,
  request,
}) => {
  const project = await createNode(request, `${PREFIX}long ${Date.now()}`, { node_type: 'project' });
  await page.goto(`/nodes/${project.id}`);
  await expect(history(page).getByTestId('history-empty')).toBeVisible();

  // Seven notes with a change after the third: five notes show first, the
  // change between them rides along, and the rest wait behind "Show more".
  for (let n = 1; n <= 3; n++) await addNote(request, project.id, `note ${n}`);
  await request.put(`/api/nodes/${project.id}`, { data: { project_status: 'Blocked' } });
  for (let n = 4; n <= 7; n++) await addNote(request, project.id, `note ${n}`);
  await page.reload();
  await expect.poll(() => order(page)).toEqual([
    'note',
    'note',
    'note',
    'note',
    'status_changed',
    'note',
  ]);
  await expect(history(page).getByTestId('history-empty')).toHaveCount(0);
  const more = history(page).getByTestId('history-show-more');
  await expect(more).toHaveText('Show 2 more…');
  await more.click();
  await expect.poll(async () => (await order(page)).length).toBe(8);
  await expect(more).toHaveText('Show less');
});

test('the Notes feed marks an edited note and offers Delete only where it works', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const node = await createNode(request, `${PREFIX}feed ${stamp}`);
  const plain = await addNote(request, node.id, `feed plain ${stamp}`);
  const changed = await addNote(request, node.id, `feed changed ${stamp}`);
  expect((await edit(request, changed.id, `feed changed ${stamp}, since corrected`)).ok()).toBeTruthy();
  // A note of mine on someone else's node, where I am now only a viewer.
  const theirs = await createNode(otherRequest, `${PREFIX}feed theirs ${stamp}`);
  await shareNode(otherRequest, theirs.id, 'e2e-test-user', 'editor');
  const stranded = await addNote(request, theirs.id, `feed stranded ${stamp}`);
  await shareNode(otherRequest, theirs.id, 'e2e-test-user', 'viewer');

  await page.goto('/notes');
  const inFeed = (text: string) =>
    page.locator('main div.rounded-lg.border').filter({ hasText: text }).first();
  await expect(inFeed(plain.body)).toBeVisible();
  await expect(inFeed(`feed changed ${stamp}, since corrected`).getByTestId('note-edited')).toBeVisible();
  await expect(inFeed(plain.body).getByTestId('note-edited')).toHaveCount(0);
  await expect(inFeed(plain.body).getByRole('button', { name: 'Delete note' })).toBeVisible();
  // Still mine to read, no longer mine to remove: no button that would fail.
  await expect(inFeed(stranded.body)).toBeVisible();
  await expect(inFeed(stranded.body).getByRole('button', { name: 'Delete note' })).toHaveCount(0);
  await expect(page.getByRole('option', { name: 'Recently edited' })).toHaveCount(1);
});

test.describe('at phone width', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('History does not push the page sideways', async ({ page, request }) => {
    const stamp = Date.now();
    const long = 'cabinets-counters-and-the-long-delayed-lighting-plan-for-the-back-room';
    const project = await createNode(request, `${PREFIX}phone ${stamp}`, { node_type: 'project' });
    await addNote(request, project.id, `A note with a long word: ${long}-${long}`);
    // The longest label there can be (40 characters), with no space in it.
    const relabelled = await request.put(`/api/nodes/${project.id}`, {
      data: { project_status: `Waiting-on-${long}`.slice(0, 40) },
    });
    expect(relabelled.ok()).toBeTruthy();
    const published = await request.put(`/api/nodes/${project.id}`, { data: { status: 'published' } });
    expect(published.ok()).toBeTruthy();
    await page.goto(`/nodes/${project.id}`);
    await expect(history(page).getByTestId('history-change')).toHaveCount(2);
    const main = page.locator('main');
    await page.waitForLoadState('networkidle');
    const over = await main.evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(over, 'the page scrolls sideways').toBeLessThanOrEqual(1);
    const lineOver = await history(page)
      .getByTestId('history-change')
      .first()
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(lineOver, 'a change line is cut or scrolls').toBeLessThanOrEqual(1);
  });
});
