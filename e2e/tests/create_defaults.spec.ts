import { test, expect, type APIRequestContext, type Page } from './fixtures';

/**
 * New-node creation (#111): every create path applies the type's default
 * template; one-click entry points from the sidebar "+" and the palette.
 *
 * Templates and the create-mode draft (localStorage) are shared state, so the
 * spec wipes its own templates and the draft before AND after each attempt
 * (retry-safety, .claude/rules/e2e.md).
 */

const PREFIX = 'e2e default';
const DRAFT_KEY = 'ember_trove_create_draft';

type Template = { id: string; name: string; node_type: string; body: string; is_default: boolean };

async function wipeTemplates(request: APIRequestContext) {
  const resp = await request.get('/api/templates');
  expect(resp.ok()).toBeTruthy();
  for (const t of (await resp.json()) as Template[]) {
    if (t.name.startsWith(PREFIX)) await request.delete(`/api/templates/${t.id}`);
  }
}

async function createTemplate(
  request: APIRequestContext,
  name: string,
  nodeType: string,
  body: string,
  makeDefault: boolean,
): Promise<Template> {
  const resp = await request.post('/api/templates', {
    data: { name, description: null, node_type: nodeType, body },
  });
  expect(resp.ok()).toBeTruthy();
  const t = (await resp.json()) as Template;
  if (makeDefault) {
    const d = await request.put(`/api/templates/${t.id}/set-default`);
    expect(d.ok()).toBeTruthy();
  }
  return t;
}

async function clearDraft(page: Page) {
  await page.evaluate((k) => window.localStorage.removeItem(k), DRAFT_KEY);
}

const typeSelect = (page: Page) => page.locator('select:has(option[value="reference"])').first();
const templatePicker = (page: Page) => page.locator('select[title="Use a template"]');
const bodyEditor = (page: Page) => page.getByPlaceholder(/Write in Markdown/);

test.describe('creating a node applies the type default', () => {
  test.beforeEach(async ({ page, request }) => {
    await wipeTemplates(request);
    await page.goto('/tasks/my-day');
    // Let the WASM bundle finish loading: the test's own goto would abort it.
    await expect(page.locator('main')).toBeVisible();
    await clearDraft(page);
  });
  test.afterEach(async ({ page, request }) => {
    await wipeTemplates(request);
    await clearDraft(page);
  });

  test('sidebar + opens the editor with the type, its default template, and focus on the title', async ({
    page,
    request,
  }) => {
    const body = `## Kickoff ${Date.now()}\n\n- goal\n`;
    const tpl = await createTemplate(request, `${PREFIX} project kickoff`, 'project', body, true);

    await page.goto('/tasks/my-day');
    await page.getByTestId('new-project').click();
    await expect(page).toHaveURL(/\/nodes\/new\?type=project/);
    await expect(typeSelect(page)).toHaveValue('project');
    // Regression (#111): the editor used to ignore the default template.
    await expect(bodyEditor(page)).toHaveValue(body);
    await expect(templatePicker(page)).toHaveValue(tpl.id);
    await expect(page.getByPlaceholder('Node title…')).toBeFocused();
    // Grouped picker: this type's templates first, default starred.
    await expect(
      templatePicker(page).locator('optgroup[label="Project templates"] option', {
        hasText: `★ ${PREFIX} project kickoff`,
      }),
    ).toHaveCount(1);
  });

  test('changing type re-resolves the body only while it is untouched', async ({ page, request }) => {
    const body = `## Kickoff ${Date.now()}\n`;
    await createTemplate(request, `${PREFIX} project kickoff`, 'project', body, true);

    await page.goto('/nodes/new?type=project');
    await expect(bodyEditor(page)).toHaveValue(body);

    // Untouched → the new type's body (no Article default: built-in scaffold).
    await typeSelect(page).selectOption('article');
    await expect(bodyEditor(page)).not.toHaveValue(body);
    await expect(templatePicker(page)).toHaveValue('');
    // And back: the Project default again.
    await typeSelect(page).selectOption('project');
    await expect(bodyEditor(page)).toHaveValue(body);

    // Once the user types, type changes never touch the body.
    await bodyEditor(page).fill(`${body}my own notes`);
    await typeSelect(page).selectOption('area');
    await expect(bodyEditor(page)).toHaveValue(`${body}my own notes`);
  });

  test('"Use as default" on a picked template makes it the type default', async ({ page, request }) => {
    const picked = await createTemplate(
      request,
      `${PREFIX} project lite`,
      'project',
      '## Lite\n',
      false,
    );

    await page.goto('/nodes/new?type=project');
    await templatePicker(page).selectOption(picked.id);
    await expect(bodyEditor(page)).toHaveValue('## Lite\n');
    const useDefault = page.getByTestId('use-as-default');
    await expect(useDefault).toHaveText('★ Use as default for Project');
    await useDefault.click();
    await expect(useDefault).toHaveCount(0);

    await expect
      .poll(async () => {
        const all = (await (await request.get('/api/templates')).json()) as Template[];
        return all.find((t) => t.id === picked.id)?.is_default;
      })
      .toBe(true);
  });

  test('a second sidebar + while the editor is open switches the type', async ({ page, request }) => {
    const body = `## Kickoff ${Date.now()}\n`;
    await createTemplate(request, `${PREFIX} project kickoff`, 'project', body, true);
    await page.goto('/tasks/my-day');
    await page.getByTestId('new-project').click();
    await expect(bodyEditor(page)).toHaveValue(body);
    // Same route, new query: the mounted editor must follow (untouched body).
    await page.getByTestId('new-area').click();
    await expect(page).toHaveURL(/\/nodes\/new\?type=area/);
    await expect(typeSelect(page)).toHaveValue('area');
    await expect(bodyEditor(page)).not.toHaveValue(body);
  });

  test('palette "New Area" opens the editor with the type preset', async ({ page }) => {
    await page.goto('/tasks/my-day');
    await expect(page.locator('main')).toBeVisible({ timeout: 15_000 });
    await page.keyboard.press('ControlOrMeta+k');
    const input = page.getByPlaceholder('Search nodes, or type a new title…');
    await expect(input).toBeFocused();
    await input.fill('New Area');
    // Scope to the palette: the sidebar "+" is also named "New Area".
    // The command (not "Create node titled 'New Area'"); its accessible name
    // includes the glyph text (e2e.md: glyph names pollute accessible text).
    await page
      .getByRole('dialog')
      .getByRole('button', { name: 'category New Area', exact: true })
      .click();
    await expect(page).toHaveURL(/\/nodes\/new\?type=area/);
    await expect(typeSelect(page)).toHaveValue('area');
  });
  test('the create dialog keeps a picked template (P1 #2 review)', async ({ page, request }) => {
    // The type has a default; the person picks another template. The pick
    // sets the type, which used to re-run the default and switch the picker
    // back to it (the node then recorded the wrong template).
    await createTemplate(request, `${PREFIX} article plain`, 'article', 'Default body', true);
    const other = await createTemplate(request, `${PREFIX} article other`, 'article', 'Other body', false);
    await page.locator('main').click({ position: { x: 5, y: 5 } });
    await page.keyboard.press('n');
    await page.getByPlaceholder(/What’s on your mind/).fill('e2e default picked');
    await page.getByRole('button', { name: /More fields/ }).click();
    const dialog = page.getByRole('dialog', { name: 'Create node' });
    await expect(dialog).toBeVisible();
    const picker = dialog.locator('select:has(option:text-is("— No template —"))');
    await expect(picker.locator(`option[value="${other.id}"]`)).toHaveCount(1);
    await picker.selectOption(other.id);
    await expect(picker).toHaveValue(other.id);
    await expect(dialog.getByRole('textbox').last()).toHaveValue('Other body');
    // Give any late effect a chance to undo it.
    await page.waitForTimeout(500);
    await expect(picker).toHaveValue(other.id);
  });
});
