import { test, expect, createNode, shareNode } from './fixtures';
import type { APIRequestContext } from '@playwright/test';

/**
 * History (S6d, #329; ONTOLOGY §9.2, §10): what occurred about a node, in
 * one chronology. This file holds the rules through the API: a note's
 * "edited" mark, what a reader may do with a note, and the split of a
 * node's log into its History and its audit trail.
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
