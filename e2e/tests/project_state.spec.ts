import { test, expect, createNode, shareNode } from './fixtures';
import type { APIRequestContext } from '@playwright/test';

/**
 * Project states (S6a, #320; ONTOLOGY §4, §4.1): a Project is active,
 * parked, complete or cancelled. The state is declared, never read from the
 * status label, and only an active project asks for attention.
 */

const OTHER_SUBJECT = 'e2e-other-user';
const PREFIX = 'e2e pstate ';

async function declare(request: APIRequestContext, nodeId: string, state: string) {
  const resp = await request.put(`/api/nodes/${nodeId}/project-state`, { data: { state } });
  expect(resp.ok(), `declare ${state}`).toBeTruthy();
  return resp.json();
}

async function project(request: APIRequestContext, title: string, label?: string) {
  const resp = await request.post('/api/nodes', {
    data: {
      title,
      node_type: 'project',
      body: '',
      metadata: {},
      status: 'draft',
      ...(label ? { project_status: label } : {}),
    },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

/** Empty the caller's Focus: it belongs to the identity, not to a node. */
async function wipeFocus(request: APIRequestContext) {
  const resp = await request.get('/api/focus');
  expect(resp.ok()).toBeTruthy();
  for (const entry of await resp.json()) {
    await request.delete(`/api/focus/${entry.focus.node_id}`);
  }
}

/** Retry-safe (`.claude/rules/e2e.md`): remove what a crashed earlier
 * attempt left behind before starting, and everything after. */
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
  await wipe(request);
  await wipeFocus(request);
  await wipeFocus(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipe(request);
  await wipeFocus(request);
  await wipeFocus(otherRequest);
});

const focusTitles = async (request: APIRequestContext): Promise<string[]> =>
  (await (await request.get('/api/focus')).json()).map(
    (e: { project: { title: string } }) => e.project.title,
  );

test('a state is declared by an editor, dated, and written to History', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const p = await project(request, `${PREFIX}declared ${stamp}`, 'Active');
  // A new project is active, and nobody has declared that.
  expect(p.project_state).toBe('active');
  expect(p.project_state_at).toBeNull();
  // Nothing but a Project has a state.
  const article = await createNode(request, `${PREFIX}article ${stamp}`);
  expect(article.project_state).toBeNull();

  const parked = await declare(request, p.id, 'parked');
  expect(parked.project_state).toBe('parked');
  expect(parked.project_state_at).toBeTruthy();
  expect(parked.project_status, 'the label is left alone').toBe('Active');
  // Sent again (a stale tab): the same date, the same version.
  const again = await declare(request, p.id, 'parked');
  expect(again.project_state_at).toBe(parked.project_state_at);
  expect(again.updated_at).toBe(parked.updated_at);

  // A label never sets the state again: "Done" does not complete it.
  const relabelled = await request.put(`/api/nodes/${p.id}`, { data: { project_status: 'Done' } });
  expect((await relabelled.json()).project_state).toBe('parked');

  await declare(request, p.id, 'complete');
  // A copy is a new, active project: it doesn't start out labelled "Done".
  const copy = await (await request.post(`/api/nodes/${p.id}/duplicate`)).json();
  expect(copy.project_state).toBe('active');
  expect(copy.project_state_at).toBeNull();
  expect(copy.project_status).toBe('Active');
  const history: { action: string; metadata: { from?: string; to?: string } }[] = await (
    await request.get(`/api/nodes/${p.id}/activity`)
  ).json();
  const changes = history
    .filter((e) => e.action === 'project_state_changed')
    .map((e) => `${e.metadata.from} > ${e.metadata.to}`)
    .sort();
  expect(changes).toEqual(['active > parked', 'parked > complete']);

  // Only a Project, and only one of the four states.
  const onArticle = await request.put(`/api/nodes/${article.id}/project-state`, {
    data: { state: 'parked' },
  });
  expect(onArticle.status()).toBe(422);
  const noSuchState = await request.put(`/api/nodes/${p.id}/project-state`, {
    data: { state: 'on_hold' },
  });
  expect(noSuchState.status()).toBe(422);

  // A viewer may read the state and not declare one; a stranger learns
  // nothing, not even that the other node is an Article.
  await shareNode(request, p.id, OTHER_SUBJECT, 'viewer');
  expect((await (await otherRequest.get(`/api/nodes/${p.id}`)).json()).project_state).toBe(
    'complete',
  );
  for (const id of [p.id, article.id]) {
    const refused = await otherRequest.put(`/api/nodes/${id}/project-state`, {
      data: { state: 'active' },
    });
    expect(refused.status(), id).toBe(403);
  }
  expect((await (await request.get(`/api/nodes/${p.id}`)).json()).project_state).toBe('complete');
});

test('Focus shows active projects: parking hides one, closing removes it', async ({ request }) => {
  const stamp = Date.now();
  const kept = await project(request, `${PREFIX}kept ${stamp}`);
  const moving = await project(request, `${PREFIX}moving ${stamp}`);
  for (const [p, why] of [
    [kept, 'the steady one'],
    [moving, 'needs me this week'],
  ] as const) {
    const put = await request.put(`/api/focus/${p.id}`, { data: { why_now: why } });
    expect(put.ok()).toBeTruthy();
  }
  expect(await focusTitles(request)).toEqual([kept.title, moving.title]);

  // Parked: out of Focus, and it can't be put there.
  await declare(request, moving.id, 'parked');
  expect(await focusTitles(request)).toEqual([kept.title]);
  const refused = await request.put(`/api/focus/${moving.id}`, { data: {} });
  expect(refused.status()).toBe(422);
  expect(await refused.text()).toContain('parked');

  // Active again: back in its place, with its why-line.
  await declare(request, moving.id, 'active');
  const back = await (await request.get('/api/focus')).json();
  expect(back.map((e: { project: { title: string } }) => e.project.title)).toEqual([
    kept.title,
    moving.title,
  ]);
  expect(back[1].focus.why_now).toBe('needs me this week');

  // Complete: it leaves Focus, and re-opening it does not put it back.
  await declare(request, moving.id, 'complete');
  expect(await focusTitles(request)).toEqual([kept.title]);
  const closed = await request.put(`/api/focus/${moving.id}`, { data: {} });
  expect(closed.status()).toBe(422);
  expect(await closed.text()).toContain('closed');
  await declare(request, moving.id, 'active');
  expect(await focusTitles(request)).toEqual([kept.title]);
});

test('the dashboard and the Projects list follow the state', async ({ request }) => {
  const stamp = Date.now();
  const active = await project(request, `${PREFIX}active ${stamp}`);
  const parked = await project(request, `${PREFIX}parked ${stamp}`);
  const done = await project(request, `${PREFIX}done ${stamp}`);
  const dropped = await project(request, `${PREFIX}dropped ${stamp}`);
  await declare(request, parked.id, 'parked');
  await declare(request, done.id, 'complete');
  await declare(request, dropped.id, 'cancelled');

  // The dashboard: active and parked projects, each with its state. A
  // complete or cancelled one has left it.
  const cards: { node_id: string; project_state: string; project_state_at: string | null }[] =
    await (await request.get('/api/dashboard/projects')).json();
  const stateOf = (id: string) => cards.find((c) => c.node_id === id)?.project_state;
  expect(stateOf(active.id)).toBe('active');
  expect(stateOf(parked.id)).toBe('parked');
  expect(cards.find((c) => c.node_id === parked.id)?.project_state_at).toBeTruthy();
  expect(stateOf(done.id)).toBeUndefined();
  expect(stateOf(dropped.id)).toBeUndefined();

  // The Projects list filters on the state, server side.
  const listed = async (state: string): Promise<string[]> => {
    const resp = await request.get(
      `/api/nodes?node_type=project&project_state=${state}&per_page=200`,
    );
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    return body.nodes.map((n: { id: string }) => n.id);
  };
  const mine = [active.id, parked.id, done.id, dropped.id];
  for (const [state, id] of [
    ['active', active.id],
    ['parked', parked.id],
    ['complete', done.id],
    ['cancelled', dropped.id],
  ] as const) {
    const ids = await listed(state);
    expect(
      mine.filter((m) => ids.includes(m)),
      state,
    ).toEqual([id]);
  }
});

test('Data health lists a label that reads as another state until one is declared', async ({
  request,
}) => {
  const stamp = Date.now();
  const rule = 'label_reads_as_another_state';
  type Item = {
    subject: { id: string };
    label_reading?: { label: string; reads_as: string; state: string };
  };
  const items = async (): Promise<Item[]> => {
    const report = await (await request.get('/api/data-health')).json();
    return report.rules.find((r: { rule: string }) => r.rule === rule)?.items ?? [];
  };
  const onHold = await project(request, `${PREFIX}on hold ${stamp}`, 'On hold');
  const plain = await project(request, `${PREFIX}plain ${stamp}`, 'Active');

  const found = (await items()).find((i) => i.subject.id === onHold.id);
  expect(found?.label_reading).toEqual({ label: 'On hold', reads_as: 'parked', state: 'active' });
  expect((await items()).some((i) => i.subject.id === plain.id)).toBe(false);

  // Keeping the state it has is a declaration too: the item is settled.
  await declare(request, onHold.id, 'active');
  expect((await items()).some((i) => i.subject.id === onHold.id)).toBe(false);
});
