import { test, expect, createNode, shareNode } from './fixtures';

/**
 * The Project Brief (#282): where a project stands now. A section on the
 * Project page, shown on the dashboard card, with its age; the body's
 * Status section can move into it.
 */

const OTHER_SUBJECT = 'e2e-other-user';

async function putBrief(
  request: import('@playwright/test').APIRequestContext,
  nodeId: string,
  data: Record<string, unknown>,
) {
  const resp = await request.put(`/api/nodes/${nodeId}/brief`, { data });
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

test('a brief is written, shown with its age, edited and removed', async ({ page, request }) => {
  const stamp = Date.now();
  const node = await createNode(request, `e2e brief ${stamp}`, {
    node_type: 'project',
    body: 'Durable notes about the project.',
  });
  try {
    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-brief');
    await expect(section.getByTestId('brief-empty')).toBeVisible();

    await section.getByTestId('brief-empty').getByRole('button', { name: 'Write brief' }).click();
    const editor = section.getByTestId('brief-editor');
    await expect(editor.getByLabel('Headline')).toBeFocused();

    // An empty brief is refused in the form, in a sentence.
    await editor.getByRole('button', { name: 'Save' }).click();
    await expect(editor.getByTestId('brief-error')).toHaveText(
      'A brief needs a headline, a pitch or at least one point.',
    );

    await editor.getByLabel('Headline').fill(`Waiting on the vendor ${stamp}`);
    await editor.getByLabel('Pitch').fill('The pilot is **agreed**; the review is not.');
    await editor.getByLabel('Points').fill('- Review booked for Friday\n\nSend the scope');
    await editor.getByRole('button', { name: 'Save' }).click();

    const brief = section.getByTestId('brief');
    await expect(brief.getByTestId('brief-headline')).toHaveText(`Waiting on the vendor ${stamp}`);
    // Markdown is rendered; a typed list marker is not kept.
    await expect(brief.getByTestId('brief-pitch').locator('strong')).toHaveText('agreed');
    await expect(brief.getByTestId('brief-points').locator('li')).toHaveText([
      'Review booked for Friday',
      'Send the scope',
    ]);
    await expect(brief.getByTestId('brief-age')).toContainText('Updated just now');
    await expect(brief.getByTestId('brief-stale')).toHaveCount(0);

    // The Brief leads the page: it sits above the body.
    const briefTop = (await section.boundingBox())?.y ?? Infinity;
    const bodyTop = (await page.getByTestId('node-body').boundingBox())?.y ?? -Infinity;
    expect(briefTop).toBeLessThan(bodyTop);

    // It is stored, not just shown.
    await page.reload();
    await expect(section.getByTestId('brief-headline')).toHaveText(
      `Waiting on the vendor ${stamp}`,
    );

    // The dashboard card leads with it.
    await page.goto('/dashboard');
    const card = page.getByTestId('card-brief').filter({ hasText: `Waiting on the vendor ${stamp}` });
    await expect(card).toBeVisible();
    await expect(card.getByTestId('brief-points').locator('li')).toHaveCount(2);

    // Edit, then remove.
    await page.goto(`/nodes/${node.id}`);
    await section.getByRole('button', { name: 'Edit brief' }).click();
    await expect(editor.getByLabel('Pitch')).toHaveValue(
      'The pilot is **agreed**; the review is not.',
    );
    await expect(editor.getByLabel('Points')).toHaveValue(
      'Review booked for Friday\nSend the scope',
    );
    await editor.getByLabel('Headline').fill(`Review passed ${stamp}`);
    await editor.getByLabel('Headline').press('ControlOrMeta+Enter');
    await expect(section.getByTestId('brief-headline')).toHaveText(`Review passed ${stamp}`);

    await section.getByRole('button', { name: 'Edit brief' }).click();
    await editor.getByRole('button', { name: 'Remove brief' }).click();
    await editor.getByTestId('brief-remove-confirm').getByRole('button', { name: 'Remove' }).click();
    await expect(section.getByTestId('brief-empty')).toBeVisible();
    const stored = await request.get(`/api/nodes/${node.id}/brief`);
    expect(await stored.json()).toBeNull();
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test("the body's Status section moves into the brief and leaves the body", async ({
  page,
  request,
}) => {
  const body = [
    '# Plan',
    '',
    'Goals stay here.',
    '',
    '## Status',
    '',
    // Not a bare one-line label: the API would file that as the project's
    // status (`project_status::split_from_body`).
    'Waiting on the **vendor** review.',
    '',
    '- Review booked',
    '- Scope sent',
    '',
    '## Background',
    '',
    'Kept as written.',
    '',
  ].join('\n');
  const node = await createNode(request, `e2e brief move ${Date.now()}`, {
    node_type: 'project',
    body,
  });
  try {
    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-brief');
    await section.getByTestId('brief-empty').getByRole('button', { name: 'Write brief' }).click();
    const editor = section.getByTestId('brief-editor');
    const offer = editor.getByTestId('brief-status-offer');
    await expect(offer).toContainText("This project's body has a Status section.");

    await offer.getByRole('button', { name: 'Move it into the brief' }).click();
    await expect(editor.getByLabel('Pitch')).toHaveValue('Waiting on the **vendor** review.');
    await expect(editor.getByLabel('Points')).toHaveValue('Review booked\nScope sent');
    await expect(offer).toContainText('The Status section will leave the body when you save.');

    await editor.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('The Status section moved out of the body.')).toBeVisible();
    await expect(section.getByTestId('brief-points').locator('li')).toHaveText([
      'Review booked',
      'Scope sent',
    ]);

    // One home: the section is gone from the body, the rest is untouched.
    const after = await (await request.get(`/api/nodes/${node.id}`)).json();
    expect(after.body).toBe('# Plan\n\nGoals stay here.\n\n## Background\n\nKept as written.\n');
    const brief = await (await request.get(`/api/nodes/${node.id}/brief`)).json();
    expect(brief.pitch).toBe('Waiting on the **vendor** review.');

    // The page shows the new body, and there is nothing left to offer.
    await expect(page.getByRole('heading', { name: 'Background' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Status', exact: true })).toHaveCount(0);
    await section.getByRole('button', { name: 'Edit brief' }).click();
    await expect(editor.getByTestId('brief-status-offer')).toHaveCount(0);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('a brief that has not been updated for two weeks says so', async ({ page, request }) => {
  const node = await createNode(request, `e2e brief stale ${Date.now()}`, {
    node_type: 'project',
  });
  try {
    const old = new Date(Date.now() - 20 * 24 * 60 * 60 * 1000).toISOString();
    await page.route(`**/api/nodes/${node.id}/brief`, (route) =>
      route.fulfill({
        json: {
          node_id: node.id,
          headline: 'An old story',
          pitch: '',
          points: [],
          author_id: 'e2e-test-user',
          stated_at: old,
          updated_at: old,
        },
      }),
    );
    await page.goto(`/nodes/${node.id}`);
    const brief = page.getByTestId('node-brief').getByTestId('brief');
    await expect(brief.getByTestId('brief-headline')).toHaveText('An old story');
    await expect(brief.getByTestId('brief-stale')).toHaveText(/May be out of date/);
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test("a save over someone else's change is refused and keeps the typed text", async ({
  page,
  request,
}) => {
  const node = await createNode(request, `e2e brief conflict ${Date.now()}`, {
    node_type: 'project',
  });
  try {
    const first = await putBrief(request, node.id, { headline: 'First', pitch: '', points: [] });
    await page.goto(`/nodes/${node.id}`);
    const section = page.getByTestId('node-brief');
    await section.getByRole('button', { name: 'Edit brief' }).click();
    const editor = section.getByTestId('brief-editor');
    await editor.getByLabel('Headline').fill('Mine, typed slowly');

    // Meanwhile the brief moves on.
    await putBrief(request, node.id, {
      headline: 'Theirs',
      pitch: '',
      points: [],
      expected_updated_at: first.updated_at,
    });

    await editor.getByRole('button', { name: 'Save' }).click();
    await expect(editor.getByTestId('brief-error')).toContainText(
      'Someone else changed this brief while you were editing',
    );
    await expect(editor.getByLabel('Headline')).toHaveValue('Mine, typed slowly');
    const stored = await (await request.get(`/api/nodes/${node.id}/brief`)).json();
    expect(stored.headline).toBe('Theirs');

    await editor.getByRole('button', { name: 'Load the latest' }).click();
    await expect(section.getByTestId('brief-headline')).toHaveText('Theirs');
  } finally {
    await request.delete(`/api/nodes/${node.id}`);
  }
});

test('only a project has a brief, and a viewer only reads it', async ({ page, request }) => {
  const stamp = Date.now();
  const article = await createNode(request, `e2e brief article ${stamp}`);
  const project = await createNode(request, `e2e brief shared ${stamp}`, {
    node_type: 'project',
  });
  try {
    await page.goto(`/nodes/${article.id}`);
    await expect(page.getByTestId('node-notes')).toBeVisible();
    await expect(page.getByTestId('node-brief')).toHaveCount(0);

    await putBrief(request, project.id, { headline: 'Shared story', pitch: '', points: [] });
    await shareNode(request, project.id, OTHER_SUBJECT, 'viewer');
    await page.setExtraHTTPHeaders({ 'x-e2e-subject': 'other' });
    await page.goto(`/nodes/${project.id}`);
    const section = page.getByTestId('node-brief');
    await expect(section.getByTestId('brief-headline')).toHaveText('Shared story');
    await expect(section.getByRole('button', { name: 'Edit brief' })).toHaveCount(0);
  } finally {
    await page.setExtraHTTPHeaders({});
    await request.delete(`/api/nodes/${article.id}`);
    await request.delete(`/api/nodes/${project.id}`);
  }
});
