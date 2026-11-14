import { test, expect, shareNode, E2E_SUBJECT } from './fixtures';
import type { APIRequestContext } from '@playwright/test';

/**
 * The cascade (S6b and S6c, #326; ONTOLOGY §4.1): declaring a Project
 * complete or cancelled closes its sub-Projects and cancels the tasks still
 * to do under it, once the plan has been shown and confirmed, and only
 * through the declaring person's own grants.
 */

const OTHER_SUBJECT = 'e2e-other-user';
const PREFIX = 'e2e cascade ';

type Plan = {
  state: string;
  own_grant: boolean;
  projects: { id: string; title: string; from: string }[];
  tasks: { node_id: string; node_title: string; tasks: { id: string; others: boolean }[] }[];
  skipped: { id: string; title: string }[];
  token: string;
};

async function project(request: APIRequestContext, title: string) {
  const resp = await request.post('/api/nodes', {
    data: { title, node_type: 'project', body: '', metadata: {}, status: 'draft' },
  });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

async function addTask(request: APIRequestContext, nodeId: string, title: string) {
  const resp = await request.post(`/api/nodes/${nodeId}/tasks`, { data: { title } });
  expect(resp.ok(), `task ${title}`).toBeTruthy();
  return resp.json();
}

/** Give `child` the home `home`. The caller must be an editor of both. */
async function under(request: APIRequestContext, home: string, child: string) {
  const resp = await request.post('/api/edges', {
    data: { source_id: home, target_id: child, edge_type: 'contains' },
  });
  expect(resp.ok(), 'contains edge').toBeTruthy();
}

const planFor = (request: APIRequestContext, id: string, state: string) =>
  request.get(`/api/nodes/${id}/project-state/cascade?state=${state}`);

const declare = (request: APIRequestContext, id: string, state: string, confirm?: string) =>
  request.put(`/api/nodes/${id}/project-state`, {
    data: confirm === undefined ? { state } : { state, confirm },
  });

const stateOf = async (request: APIRequestContext, id: string): Promise<string> =>
  (await (await request.get(`/api/nodes/${id}`)).json()).project_state;

const tasksOn = async (
  request: APIRequestContext,
  id: string,
): Promise<{ id: string; title: string; status: string; completed_at: string | null }[]> =>
  (await request.get(`/api/nodes/${id}/tasks`)).json();

/** Retry-safe (`.claude/rules/e2e.md`): each identity removes its own. */
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

test('closing a project closes what is under it, once its plan is confirmed', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const master = await project(request, `${PREFIX}master ${stamp}`);
  const child = await project(request, `${PREFIX}child ${stamp}`);
  const deep = await project(request, `${PREFIX}grandchild ${stamp}`);
  await under(request, master.id, child.id);
  await under(request, child.id, deep.id);
  const mine = await addTask(request, master.id, `${PREFIX}mine ${stamp}`);
  const onDeep = await addTask(request, deep.id, `${PREFIX}deep ${stamp}`);

  // The other person may edit the master. Their task on it is "someone
  // else's" in the plan. Their own project under it is shared for viewing
  // only, so it stays open and is said; another of theirs is not shared,
  // and is not in the plan at all.
  await shareNode(request, master.id, OTHER_SUBJECT, 'editor');
  const theirTask = await addTask(otherRequest, master.id, `${PREFIX}theirs ${stamp}`);
  const viewed = await project(otherRequest, `${PREFIX}viewed ${stamp}`);
  const hidden = await project(otherRequest, `${PREFIX}hidden ${stamp}`);
  await under(otherRequest, master.id, viewed.id);
  await under(otherRequest, master.id, hidden.id);
  await shareNode(otherRequest, viewed.id, E2E_SUBJECT, 'viewer');
  const viewedTask = await addTask(otherRequest, viewed.id, `${PREFIX}on viewed ${stamp}`);

  const shown = await planFor(request, master.id, 'complete');
  expect(shown.ok()).toBeTruthy();
  const plan: Plan = await shown.json();
  expect(plan.state).toBe('complete');
  expect(plan.own_grant).toBe(true);
  expect(plan.projects.map((p) => p.id)).toEqual([child.id, deep.id]);
  expect(plan.projects.every((p) => p.from === 'active')).toBe(true);
  const planned = plan.tasks.flatMap((g) => g.tasks);
  expect(planned.map((t) => t.id).sort()).toEqual([mine.id, onDeep.id, theirTask.id].sort());
  expect(planned.filter((t) => t.others).map((t) => t.id)).toEqual([theirTask.id]);
  expect(plan.skipped.map((s) => s.id)).toEqual([viewed.id]);
  const text = JSON.stringify(plan);
  expect(text, 'a hidden project is not in the plan').not.toContain(hidden.id);
  expect(text).not.toContain('hidden');
  expect(text, 'nor a task under a skipped one').not.toContain(viewedTask.id);
  expect(plan.token).toMatch(/^[0-9a-f]{64}$/);

  // Only complete and cancelled have a plan.
  expect((await planFor(request, master.id, 'parked')).status()).toBe(422);

  // Unconfirmed, or confirmed with something else: 412, nothing written.
  for (const confirm of [undefined, 'not-the-plan']) {
    const refused = await declare(request, master.id, 'complete', confirm);
    expect(refused.status(), `confirm: ${confirm}`).toBe(412);
  }
  expect(await stateOf(request, master.id)).toBe('active');
  expect(await stateOf(request, child.id)).toBe('active');

  // The plan changed underneath: a task was added. The old token no
  // longer confirms it.
  const late = await addTask(request, child.id, `${PREFIX}late ${stamp}`);
  expect((await declare(request, master.id, 'complete', plan.token)).status()).toBe(412);
  expect(await stateOf(request, master.id)).toBe('active');
  const fresh: Plan = await (await planFor(request, master.id, 'complete')).json();
  expect(fresh.token).not.toBe(plan.token);

  const closed = await declare(request, master.id, 'complete', fresh.token);
  expect(closed.ok()).toBeTruthy();
  for (const id of [master.id, child.id, deep.id]) {
    expect(await stateOf(request, id), id).toBe('complete');
  }
  // What the actor may not edit stays open, with its task.
  expect(await stateOf(otherRequest, viewed.id)).toBe('active');
  expect(await stateOf(otherRequest, hidden.id)).toBe('active');
  expect((await tasksOn(otherRequest, viewed.id))[0].status).toBe('open');
  // Cancelled, not done: no completion date.
  for (const [node, id] of [
    [master.id, mine.id],
    [master.id, theirTask.id],
    [child.id, late.id],
    [deep.id, onDeep.id],
  ]) {
    const task = (await tasksOn(request, node)).find((t) => t.id === id);
    expect(task?.status, id).toBe('cancelled');
    expect(task?.completed_at, id).toBeNull();
  }

  // History on the master. Its owner is told which sub-Projects closed;
  // the other editor can't view them, so is told only about the tasks.
  type Entry = {
    action: string;
    metadata: {
      state?: string;
      sub_projects?: { id: string; title: string }[];
      tasks?: { id: string }[];
      tasks_total?: number;
      root?: { id: string; title: string };
    };
  };
  const cascadeOn = async (as: APIRequestContext, id: string): Promise<Entry[]> =>
    ((await (await as.get(`/api/nodes/${id}/activity`)).json()) as Entry[]).filter(
      (e) => e.action === 'cascade_closed',
    );
  const forOwner = await cascadeOn(request, master.id);
  expect(forOwner).toHaveLength(1);
  expect(forOwner[0].metadata.state).toBe('complete');
  expect(forOwner[0].metadata.sub_projects?.map((p) => p.title).sort()).toEqual(
    [child.title, deep.title].sort(),
  );
  expect(forOwner[0].metadata.tasks_total).toBe(2);
  const forOther = await cascadeOn(otherRequest, master.id);
  expect(forOther).toHaveLength(1);
  // Not even an empty list: that would say there were some.
  expect(forOther[0].metadata.sub_projects).toBeUndefined();
  expect(forOther[0].metadata.tasks_total).toBe(2);
  expect(JSON.stringify(forOther)).not.toContain(child.id);
  expect(JSON.stringify(forOther)).not.toContain('grandchild');
  // On the child: its own record names the master it came from.
  const onChild = await cascadeOn(request, child.id);
  expect(onChild).toHaveLength(1);
  expect(onChild[0].metadata.root?.id).toBe(master.id);

  // Sent again (a double click, a retry): quiet, and nothing new.
  expect((await declare(request, master.id, 'complete', fresh.token)).ok()).toBeTruthy();
  expect(await cascadeOn(request, master.id)).toHaveLength(1);
});

test('History shows a cascade only to a reader who can see something it closed', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const master = await project(request, `${PREFIX}quiet master ${stamp}`);
  const child = await project(request, `${PREFIX}quiet child ${stamp}`);
  await under(request, master.id, child.id);
  // The other person may view the master, and not the child.
  await shareNode(request, master.id, OTHER_SUBJECT, 'viewer');
  const plan: Plan = await (await planFor(request, master.id, 'complete')).json();
  expect((await declare(request, master.id, 'complete', plan.token)).ok()).toBeTruthy();

  const actions = async (as: APIRequestContext): Promise<string[]> =>
    ((await (await as.get(`/api/nodes/${master.id}/activity`)).json()) as { action: string }[]).map(
      (e) => e.action,
    );
  expect(await actions(request)).toContain('cascade_closed');
  // "Closed what was under it", with nothing beside it, would tell them a
  // project they can't see exists. They see the master's own change only.
  const theirs = await actions(otherRequest);
  expect(theirs).toContain('project_state_changed');
  expect(theirs).not.toContain('cascade_closed');
});

test('rights come before the confirmation: a stranger learns nothing from it', async ({
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const master = await project(request, `${PREFIX}private ${stamp}`);
  await addTask(request, master.id, `${PREFIX}task ${stamp}`);
  const plan: Plan = await (await planFor(request, master.id, 'cancelled')).json();

  // No grant: 403 for the plan, and for the write with or without the
  // real token (never the 412 that would say there is something under it).
  expect((await planFor(otherRequest, master.id, 'cancelled')).status()).toBe(403);
  for (const confirm of [undefined, plan.token, 'guess']) {
    expect((await declare(otherRequest, master.id, 'cancelled', confirm)).status()).toBe(403);
  }
  // A viewer: the same.
  await shareNode(request, master.id, OTHER_SUBJECT, 'viewer');
  expect((await planFor(otherRequest, master.id, 'cancelled')).status()).toBe(403);
  expect((await declare(otherRequest, master.id, 'cancelled', plan.token)).status()).toBe(403);
  expect(await stateOf(request, master.id)).toBe('active');
});

test('re-opening is the master alone, and no sub-project re-opens under a closed one', async ({
  request,
}) => {
  const stamp = Date.now();
  const master = await project(request, `${PREFIX}reopen master ${stamp}`);
  const child = await project(request, `${PREFIX}reopen child ${stamp}`);
  await under(request, master.id, child.id);
  const task = await addTask(request, child.id, `${PREFIX}reopen task ${stamp}`);
  const plan: Plan = await (await planFor(request, master.id, 'complete')).json();
  expect((await declare(request, master.id, 'complete', plan.token)).ok()).toBeTruthy();

  // Neither active nor parked while the master is closed.
  for (const state of ['active', 'parked']) {
    const refused = await declare(request, child.id, state);
    expect(refused.status(), state).toBe(422);
    expect(await refused.text()).toContain('its master project is closed');
  }
  expect(await stateOf(request, child.id)).toBe('complete');
  // From one closed state to the other re-opens nothing.
  expect((await declare(request, child.id, 'cancelled')).ok()).toBeTruthy();

  // The master re-opens alone: no confirmation, and nothing under it moves.
  expect((await declare(request, master.id, 'active')).ok()).toBeTruthy();
  expect(await stateOf(request, child.id)).toBe('cancelled');
  expect((await tasksOn(request, child.id)).find((t) => t.id === task.id)?.status).toBe(
    'cancelled',
  );
  // Now the child may be re-opened, deliberately.
  expect((await declare(request, child.id, 'active')).ok()).toBeTruthy();
});

test('Data health lists what is still open under a closed project until it is closed', async ({
  request,
}) => {
  const stamp = Date.now();
  const rule = 'open_under_closed_project';
  type Item = {
    subject: { id: string };
    still_open?: { state: string; tasks: number; sub_projects: number; can_close: boolean };
  };
  const item = async (id: string): Promise<Item | undefined> => {
    const report = await (await request.get('/api/data-health')).json();
    const items: Item[] = report.rules.find((r: { rule: string }) => r.rule === rule)?.items ?? [];
    return items.find((i) => i.subject.id === id);
  };

  // Closed with nothing under it: no confirmation needed, and not listed.
  const master = await project(request, `${PREFIX}health ${stamp}`);
  expect((await declare(request, master.id, 'cancelled')).ok()).toBeTruthy();
  expect(await item(master.id)).toBeUndefined();

  // Then something open arrives under it: a task, and a sub-Project moved
  // there (allowed, and listed).
  const task = await addTask(request, master.id, `${PREFIX}left open ${stamp}`);
  const moved = await project(request, `${PREFIX}moved in ${stamp}`);
  await under(request, master.id, moved.id);
  expect((await item(master.id))?.still_open).toEqual({
    state: 'cancelled',
    tasks: 1,
    sub_projects: 1,
    can_close: true,
  });

  // Declaring the state it already has is still a no-op (Data health's
  // "Keep" sends exactly that): the catch-up is asked for, never forced.
  expect((await declare(request, master.id, 'cancelled')).ok()).toBeTruthy();
  expect((await tasksOn(request, master.id)).find((t) => t.id === task.id)?.status).toBe('open');
  expect(await stateOf(request, moved.id)).toBe('active');

  // Asked for: the same request with the plan's token.
  const plan: Plan = await (await planFor(request, master.id, 'cancelled')).json();
  expect(plan.projects.map((p) => p.id)).toEqual([moved.id]);
  expect((await declare(request, master.id, 'cancelled', plan.token)).ok()).toBeTruthy();
  expect((await tasksOn(request, master.id)).find((t) => t.id === task.id)?.status).toBe(
    'cancelled',
  );
  expect(await stateOf(request, moved.id)).toBe('cancelled');
  expect(await item(master.id)).toBeUndefined();
});
