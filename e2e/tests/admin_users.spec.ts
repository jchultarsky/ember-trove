import { test, expect, type Page } from './fixtures';

/**
 * The admin users page: suspend, reactivate and delete (#239).
 *
 * The e2e stack has no Cognito and no bypass identity is an admin (on
 * purpose), so this spec stubs `/api/auth/me` and `/api/admin/users*`.
 * The real behaviour of those routes is covered by the API's stub and
 * Postgres tests; this checks what the page shows and sends.
 */

const ME = '0a0a0a0a-0000-4000-8000-00000000000a';
const ANN = '00000000-0000-4000-8000-000000000001';
const BO = '00000000-0000-4000-8000-000000000002';

type User = { id: string; email: string; firstName: string; suspended: boolean };

function dto(u: User) {
  return {
    id: u.id,
    username: u.email,
    email: u.email,
    firstName: u.firstName,
    lastName: '',
    enabled: !u.suspended,
    realmRoles: u.id === ME ? ['admin'] : ['user'],
    suspended: u.suspended,
  };
}

const COUNTS = {
  nodes: 3,
  sharedNodes: 1,
  notes: 2,
  tasks: 0,
  tasksHandedOver: 2,
  captures: 0,
  people: 0,
  files: 1,
  settings: 0,
};

/** Stub the admin API. Returns the calls the page made. */
async function stubAdmin(
  page: Page,
  opts: { failSuspend?: boolean; failPreview?: boolean; failDelete?: boolean } = {},
) {
  const users: User[] = [
    { id: ME, email: 'me@example.com', firstName: 'Me', suspended: false },
    { id: ANN, email: 'ann@example.com', firstName: 'Ann', suspended: false },
    { id: BO, email: 'bo@example.com', firstName: 'Bo', suspended: true },
  ];
  const calls: string[] = [];
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      json: { sub: ME, email: 'me@example.com', name: 'Me', roles: ['admin'] },
    }),
  );
  await page.route('**/api/admin/users/roles', (route) => route.fulfill({ json: ['admin', 'user'] }));
  await page.route('**/api/admin/users', (route) => route.fulfill({ json: users.map(dto) }));
  await page.route(/\/api\/admin\/users\/[0-9a-f-]+(\/[a-z-]+)?$/, (route) => {
    const req = route.request();
    const [, id, action] = new URL(req.url()).pathname.match(/users\/([0-9a-f-]+)(?:\/([a-z-]+))?$/)!;
    calls.push(`${req.method()} ${id}${action ? ' ' + action : ''}`);
    const user = users.find((u) => u.id === id);
    if (action === 'suspend') {
      if (user) user.suspended = true;
      if (opts.failSuspend) {
        return route.fulfill({
          status: 502,
          json: { error: "The account is suspended in Ember Trove, but the sign-in provider didn't confirm. Try again." },
        });
      }
      return route.fulfill({ status: 204 });
    }
    if (action === 'reactivate') {
      if (user) user.suspended = false;
      return route.fulfill({ status: 204 });
    }
    if (action === 'deletion-preview') {
      if (opts.failPreview) return route.fulfill({ status: 500, json: { error: 'database error' } });
      return route.fulfill({ json: COUNTS });
    }
    if (req.method() === 'DELETE' && opts.failDelete) {
      // The API suspends first; a later step failed.
      if (user) user.suspended = true;
      return route.fulfill({ status: 500, json: { error: 'internal error' } });
    }
    if (req.method() === 'DELETE') {
      users.splice(users.indexOf(user!), 1);
      return route.fulfill({ json: COUNTS });
    }
    return route.fallback();
  });
  return calls;
}

function row(page: Page, email: string) {
  return page.getByTestId('user-row').filter({ hasText: email });
}

async function gotoAdmin(page: Page) {
  await page.goto('/admin/users');
  await expect(row(page, 'ann@example.com')).toBeVisible({ timeout: 15_000 });
}

test('suspended accounts are marked, and your own row has no lockout actions', async ({ page }) => {
  await stubAdmin(page);
  await gotoAdmin(page);
  await expect(row(page, 'bo@example.com').getByTestId('suspended-badge')).toHaveText('Suspended');
  await expect(row(page, 'ann@example.com').getByTestId('suspended-badge')).toHaveCount(0);
  await expect(row(page, 'bo@example.com').getByRole('button', { name: 'Reactivate account' })).toBeVisible();
  await expect(row(page, 'ann@example.com').getByRole('button', { name: 'Suspend account' })).toBeVisible();

  const mine = row(page, 'me@example.com');
  await expect(mine.getByRole('button', { name: 'Suspend account' })).toHaveCount(0);
  await expect(mine.getByRole('button', { name: 'Delete account', exact: true })).toHaveCount(0);
  await mine.locator('button[title="Edit roles"]').click();
  await expect(mine.getByRole('checkbox').first()).toBeDisabled(); // admin
});

test('suspend and reactivate call the API and update the row', async ({ page }) => {
  const calls = await stubAdmin(page);
  await gotoAdmin(page);
  await row(page, 'ann@example.com').getByRole('button', { name: 'Suspend account' }).click();
  await expect(row(page, 'ann@example.com').getByTestId('suspended-badge')).toBeVisible();
  await row(page, 'bo@example.com').getByRole('button', { name: 'Reactivate account' }).click();
  await expect(row(page, 'bo@example.com').getByTestId('suspended-badge')).toHaveCount(0);
  expect(calls).toEqual([`POST ${ANN} suspend`, `POST ${BO} reactivate`]);
});

test('a suspend Cognito did not confirm says so and shows the account suspended', async ({ page }) => {
  await stubAdmin(page, { failSuspend: true });
  await gotoAdmin(page);
  await row(page, 'ann@example.com').getByRole('button', { name: 'Suspend account' }).click();
  await expect(row(page, 'ann@example.com').getByRole('alert')).toContainText('suspended in Ember Trove');
  await expect(row(page, 'ann@example.com').getByTestId('suspended-badge')).toBeVisible();
});

test('deleting shows what goes and needs the email typed', async ({ page }) => {
  const calls = await stubAdmin(page);
  await gotoAdmin(page);
  await row(page, 'ann@example.com').getByRole('button', { name: 'Delete account', exact: true }).click();

  const dialog = page.getByRole('alertdialog', { name: 'Delete account' });
  const preview = dialog.getByTestId('deletion-preview');
  await expect(preview).toContainText('everything they own: 3 nodes, 2 notes and 1 file.');
  await expect(preview).toContainText('1 of those nodes is shared with other people, who lose access.');
  await expect(preview).toContainText("2 tasks they added to other people's Projects pass to those Projects' owners.");
  await expect(preview).toContainText('Backups taken before now still hold this data');

  const commit = dialog.getByRole('button', { name: /Delete account and data/ });
  const confirm = dialog.getByRole('textbox', { name: "Type the account's email to confirm" });
  await expect(commit).toBeDisabled();
  await confirm.fill('ann@example');
  await expect(commit).toBeDisabled();
  await confirm.fill(' ANN@example.com ');
  await expect(commit).toBeEnabled();
  expect(calls).toEqual([`GET ${ANN} deletion-preview`]);

  await commit.click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText("Deleted Ann's account and 3 nodes, 2 notes and 1 file")).toBeVisible();
  await expect(row(page, 'ann@example.com')).toHaveCount(0);
  expect(calls).toEqual([`GET ${ANN} deletion-preview`, `DELETE ${ANN}`]);
});

test('Cancel and Escape leave the account alone', async ({ page }) => {
  const calls = await stubAdmin(page);
  await gotoAdmin(page);
  const open = () =>
    row(page, 'ann@example.com').getByRole('button', { name: 'Delete account', exact: true }).click();
  const dialog = page.getByRole('alertdialog', { name: 'Delete account' });

  await open();
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveCount(0);
  await open();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  expect(calls.filter((c) => c.startsWith('DELETE'))).toEqual([]);
  await expect(row(page, 'ann@example.com')).toBeVisible();
});

test('no delete without the counts', async ({ page }) => {
  await stubAdmin(page, { failPreview: true });
  await gotoAdmin(page);
  await row(page, 'ann@example.com').getByRole('button', { name: 'Delete account', exact: true }).click();
  const dialog = page.getByRole('alertdialog', { name: 'Delete account' });
  await expect(dialog.getByRole('alert')).toContainText("Couldn't count what this account owns");
  await dialog.getByRole('textbox', { name: "Type the account's email to confirm" }).fill('ann@example.com');
  await expect(dialog.getByRole('button', { name: /Delete account and data/ })).toBeDisabled();
});

test('a failed delete says so, and closing shows the account suspended', async ({ page }) => {
  const calls = await stubAdmin(page, { failDelete: true });
  await gotoAdmin(page);
  await row(page, 'ann@example.com').getByRole('button', { name: 'Delete account', exact: true }).click();
  const dialog = page.getByRole('alertdialog', { name: 'Delete account' });
  await dialog.getByRole('textbox', { name: "Type the account's email to confirm" }).fill('ann@example.com');
  const commit = dialog.getByRole('button', { name: /Delete account and data/ });
  await commit.dblclick();
  await expect(dialog.getByRole('alert')).toContainText('Delete failed');
  expect(calls.filter((c) => c.startsWith('DELETE'))).toEqual([`DELETE ${ANN}`]);
  // The counts are read again after the failure.
  await expect.poll(() => calls.filter((c) => c.endsWith('deletion-preview')).length).toBe(2);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(row(page, 'ann@example.com').getByTestId('suspended-badge')).toBeVisible();
});
