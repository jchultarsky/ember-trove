import { test, expect, createNode } from './fixtures';
import type { APIRequestContext } from '@playwright/test';

/**
 * Focus (#282, proposal B): the projects that hold a person's attention.
 * The dashboard's first section, in the person's own order, each card with
 * its Brief, how long it has been there, and a private why-line.
 */

const OTHER_SUBJECT = 'e2e-other-user';

/** Empty the caller's Focus. */
async function wipeFocus(request: APIRequestContext) {
  const resp = await request.get('/api/focus');
  expect(resp.ok()).toBeTruthy();
  for (const entry of await resp.json()) {
    await request.delete(`/api/focus/${entry.focus.node_id}`);
  }
}

async function putFocus(request: APIRequestContext, nodeId: string, data: object = {}) {
  const resp = await request.put(`/api/focus/${nodeId}`, { data });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

// The database outlives a failed attempt, so each one starts and ends with
// both identities' Focus empty (.claude/rules/e2e.md, "retry-safe").
test.beforeEach(async ({ request, otherRequest }) => {
  await wipeFocus(request);
  await wipeFocus(otherRequest);
});
test.afterEach(async ({ request, otherRequest }) => {
  await wipeFocus(request);
  await wipeFocus(otherRequest);
});

test('a project goes into Focus from its card, gets a why-line, is reordered and removed', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const alphaTitle = `e2e focus alpha ${stamp}`;
  const bravoTitle = `e2e focus bravo ${stamp}`;
  const alpha = await createNode(request, alphaTitle, { node_type: 'project' });
  const bravo = await createNode(request, bravoTitle, { node_type: 'project' });
  try {
    const brief = await request.put(`/api/nodes/${alpha.id}/brief`, {
      data: { headline: `Alpha stands here ${stamp}`, pitch: '', points: [] },
    });
    expect(brief.ok()).toBeTruthy();

    await page.goto('/dashboard');
    const section = page.getByTestId('focus-section');
    const groups = page.getByTestId('project-groups');
    const groupCard = (title: string) =>
      groups.getByTestId('project-card').filter({ hasText: title });
    const focusCard = (title: string) =>
      section.getByTestId('project-card').filter({ hasText: title });

    // Empty, and it says how to use it.
    await expect(page.getByTestId('focus-empty')).toBeVisible();

    // Add from the card. The click adds; it does not open the project.
    await groupCard(alphaTitle).getByRole('button', { name: 'Add to Focus' }).click();
    await expect(focusCard(alphaTitle)).toBeVisible();
    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(focusCard(alphaTitle).getByTestId('focus-age')).toContainText('In focus today');
    // The card is the project's own: it carries the Brief.
    await expect(focusCard(alphaTitle).getByTestId('card-brief')).toContainText(
      `Alpha stands here ${stamp}`,
    );
    // The card in its group now offers the way out.
    await expect(
      groupCard(alphaTitle).getByRole('button', { name: 'Remove from Focus' }),
    ).toHaveAttribute('aria-pressed', 'true');

    // The why-line: written, stored, and kept when an edit is abandoned.
    await focusCard(alphaTitle).getByRole('button', { name: 'Edit why-line' }).click();
    const why = focusCard(alphaTitle).getByLabel('Why now');
    await expect(why).toBeFocused();
    await why.fill('Decision needed Friday');
    await why.press('Enter');
    await expect(focusCard(alphaTitle).getByTestId('focus-why')).toHaveText(
      'Decision needed Friday',
    );
    await expect(page).toHaveURL(/\/dashboard$/);
    await page.reload();
    await expect(focusCard(alphaTitle).getByTestId('focus-why')).toHaveText(
      'Decision needed Friday',
    );
    await focusCard(alphaTitle).getByRole('button', { name: 'Edit why-line' }).click();
    await focusCard(alphaTitle).getByLabel('Why now').fill('never saved');
    await focusCard(alphaTitle).getByLabel('Why now').press('Escape');
    await expect(focusCard(alphaTitle).getByTestId('focus-why')).toHaveText(
      'Decision needed Friday',
    );

    // A second project goes last; the order is the person's to change.
    await groupCard(bravoTitle).getByRole('button', { name: 'Add to Focus' }).click();
    await expect(section.getByTestId('project-card')).toContainText([alphaTitle, bravoTitle]);
    await expect(focusCard(alphaTitle).getByRole('button', { name: 'Move up' })).toBeDisabled();
    await expect(focusCard(bravoTitle).getByRole('button', { name: 'Move down' })).toBeDisabled();
    await focusCard(bravoTitle).getByRole('button', { name: 'Move up' }).click();
    await expect(section.getByTestId('project-card')).toContainText([bravoTitle, alphaTitle]);
    await page.reload();
    await expect(section.getByTestId('project-card')).toContainText([bravoTitle, alphaTitle]);
    // Adding again from a page that is behind keeps the why-line.
    await putFocus(request, alpha.id);
    await page.reload();
    await expect(focusCard(alphaTitle).getByTestId('focus-why')).toHaveText(
      'Decision needed Friday',
    );

    // An emptied why-line is no why-line.
    await focusCard(alphaTitle).getByRole('button', { name: 'Edit why-line' }).click();
    await focusCard(alphaTitle).getByLabel('Why now').fill('   ');
    await focusCard(alphaTitle).getByRole('button', { name: 'Save why-line' }).click();
    await expect(focusCard(alphaTitle).getByTestId('focus-why')).toHaveCount(0);

    // Out again: gone from the section, and its card offers Focus once more.
    await focusCard(alphaTitle).getByRole('button', { name: 'Remove from Focus' }).click();
    await expect(focusCard(alphaTitle)).toHaveCount(0);
    await expect(focusCard(bravoTitle)).toBeVisible();
    await expect(
      groupCard(alphaTitle).getByRole('button', { name: 'Add to Focus' }),
    ).toHaveAttribute('aria-pressed', 'false');
  } finally {
    await request.delete(`/api/nodes/${alpha.id}`);
    await request.delete(`/api/nodes/${bravo.id}`);
  }
});

test('a project goes in and out of Focus from its own page', async ({ page, request }) => {
  const stamp = Date.now();
  const title = `e2e focus page ${stamp}`;
  const project = await createNode(request, title, { node_type: 'project' });
  const article = await createNode(request, `e2e focus article ${stamp}`);
  try {
    // Only a Project has the control.
    await page.goto(`/nodes/${article.id}`);
    await expect(page.getByTestId('node-notes')).toBeVisible();
    await expect(page.getByTestId('focus-toggle')).toHaveCount(0);

    await page.goto(`/nodes/${project.id}`);
    const toggle = page.getByTestId('focus-toggle');
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await expect(toggle).toHaveAccessibleName('Add to Focus');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-pressed', 'true');
    await expect(toggle).toHaveAccessibleName('Remove from Focus');
    // Stored, and on the dashboard.
    await page.reload();
    await expect(toggle).toHaveAttribute('aria-pressed', 'true');
    await page.goto('/dashboard');
    const card = page
      .getByTestId('focus-section')
      .getByTestId('project-card')
      .filter({ hasText: title });
    await expect(card).toBeVisible();
    // The card's title opens the project.
    await card.getByRole('button', { name: title }).click();
    await expect(page).toHaveURL(new RegExp(`/nodes/${project.id}$`));

    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await page.goto('/dashboard');
    await expect(page.getByTestId('project-groups')).toBeVisible();
    await expect(card).toHaveCount(0);

    // An archived project is in nobody's Focus, and has no control.
    const archived = await request.put(`/api/nodes/${project.id}`, {
      data: { status: 'archived' },
    });
    expect(archived.ok()).toBeTruthy();
    await page.goto(`/nodes/${project.id}`);
    await expect(page.getByTestId('node-brief')).toBeVisible();
    await expect(page.getByTestId('focus-toggle')).toHaveCount(0);
    const refused = await request.put(`/api/focus/${project.id}`, { data: {} });
    expect(refused.status()).toBe(422);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
    await request.delete(`/api/nodes/${article.id}`);
  }
});

test('Focus is private to each person and follows their access', async ({
  page,
  request,
  otherRequest,
}) => {
  const stamp = Date.now();
  const title = `e2e focus shared ${stamp}`;
  const project = await createNode(request, title, { node_type: 'project' });
  try {
    // Before it is shared, the other person can't put it in their Focus,
    // and is told what they'd be told about a project that doesn't exist.
    const hidden = await otherRequest.put(`/api/focus/${project.id}`, { data: {} });
    const missing = await otherRequest.put(
      '/api/focus/00000000-0000-4000-8000-000000000000',
      { data: {} },
    );
    expect(hidden.status()).toBe(404);
    expect(await hidden.text()).toBe(await missing.text());

    const share = await request.post(`/api/nodes/${project.id}/permissions`, {
      data: { subject_id: OTHER_SUBJECT, role: 'viewer' },
    });
    expect(share.ok()).toBeTruthy();
    const grant = await share.json();

    await putFocus(request, project.id, { why_now: `mine ${stamp}` });
    await putFocus(otherRequest, project.id, { why_now: `theirs ${stamp}` });

    // Each sees the project in their own Focus, with their own why-line.
    await page.goto('/dashboard');
    const card = page
      .getByTestId('focus-section')
      .getByTestId('project-card')
      .filter({ hasText: title });
    await expect(card.getByTestId('focus-why')).toHaveText(`mine ${stamp}`);
    await expect(page.getByText(`theirs ${stamp}`)).toHaveCount(0);

    await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
    await page.goto('/dashboard');
    await expect(card.getByTestId('focus-why')).toHaveText(`theirs ${stamp}`);
    await expect(page.getByText(`mine ${stamp}`)).toHaveCount(0);

    // The share is taken back: the project leaves their Focus, title and all.
    const revoke = await request.delete(`/api/nodes/${project.id}/permissions/${grant.id}`);
    expect(revoke.ok()).toBeTruthy();
    await page.reload();
    await expect(page.locator('main')).toBeVisible();
    await page.waitForLoadState('networkidle');
    await expect(page.getByText(title)).toHaveCount(0);
    expect(await (await otherRequest.get('/api/focus')).json()).toEqual([]);
    expect(await (await otherRequest.get(`/api/focus/${project.id}`)).json()).toBeNull();
    // The owner's own entry is untouched.
    const mine = await (await request.get('/api/focus')).json();
    expect(mine.map((e: { focus: { why_now: string } }) => e.focus.why_now)).toEqual([
      `mine ${stamp}`,
    ]);
  } finally {
    await page.setExtraHTTPHeaders({});
    await request.delete(`/api/nodes/${project.id}`);
  }
});

test('past five projects, Focus says it works best small', async ({ page, request }) => {
  const stamp = Date.now();
  const ids: string[] = [];
  try {
    for (let i = 1; i <= 6; i++) {
      const node = await createNode(request, `e2e focus many ${stamp} ${i}`, {
        node_type: 'project',
      });
      ids.push(node.id);
      await putFocus(request, node.id);
    }
    await page.goto('/dashboard');
    const section = page.getByTestId('focus-section');
    await expect(section.getByTestId('project-card')).toHaveCount(6);
    await expect(section.getByTestId('focus-hint')).toContainText('Focus works best small');

    // A hint, not a limit: at five it is gone.
    await request.delete(`/api/focus/${ids[5]}`);
    await page.reload();
    await expect(section.getByTestId('project-card')).toHaveCount(5);
    await expect(section.getByTestId('focus-hint')).toHaveCount(0);
  } finally {
    for (const id of ids) {
      await request.delete(`/api/nodes/${id}`);
    }
  }
});

test('a failed Focus load is said, and no card offers a toggle it cannot trust', async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const title = `e2e focus load ${stamp}`;
  const project = await createNode(request, title, { node_type: 'project' });
  try {
    let fail = true;
    await page.route('**/api/focus', (route) =>
      fail ? route.fulfill({ status: 500, body: 'no' }) : route.continue(),
    );
    await page.goto('/dashboard');
    const error = page.getByTestId('load-error');
    await expect(error).toContainText('Couldn’t load your Focus');
    const card = page
      .getByTestId('project-groups')
      .getByTestId('project-card')
      .filter({ hasText: title });
    await expect(card).toBeVisible();
    await expect(card.getByTestId('focus-toggle')).toHaveCount(0);

    fail = false;
    await error.getByRole('button', { name: 'Try again' }).click();
    await expect(card.getByTestId('focus-toggle')).toBeVisible();
    await expect(page.getByTestId('load-error')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${project.id}`);
  }
});
