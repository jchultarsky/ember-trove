import { test, expect } from './fixtures';

/**
 * The Backup page marks the backups the server made after a release
 * ("Automatic"), of which only the newest 10 are kept, so an admin can tell
 * them from their own (2026-11-02). Stubbed like admin_users.spec.ts: no
 * bypass identity is an admin. The pruning itself is a pg test.
 */

const ME = '0a0a0a0a-0000-4000-8000-00000000000a';

function job(id: string, createdBy: string, comment: string, minutesAgo: number) {
  return {
    id,
    created_by: createdBy,
    created_at: new Date(Date.now() - minutesAgo * 60_000).toISOString(),
    size_bytes: 1024,
    s3_key: `backups/${id}.tar.gz`,
    node_count: 1,
    edge_count: 0,
    tag_count: 0,
    note_count: 0,
    task_count: 0,
    attachment_count: 0,
    comment,
  };
}

test('automatic backups carry a badge; an admin backup does not', async ({ page }) => {
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({ json: { sub: ME, email: 'me@example.com', name: 'Me', roles: ['admin'] } }),
  );
  await page.route('**/api/admin/backups', (route) =>
    route.fulfill({
      json: [
        job('00000000-0000-4000-8000-0000000000a1', 'automatic', 'After the v9.9.9 deploy', 5),
        job('00000000-0000-4000-8000-0000000000b2', ME, 'before the upgrade', 60),
      ],
    }),
  );
  await page.goto('/admin/backup');
  const rows = page.getByTestId('backup-row');
  await expect(rows).toHaveCount(2, { timeout: 15_000 });

  const auto = rows.filter({ hasText: 'After the v9.9.9 deploy' });
  await expect(auto.getByTestId('automatic-badge')).toHaveText('Automatic');
  const mine = rows.filter({ hasText: 'before the upgrade' });
  await expect(mine.getByTestId('automatic-badge')).toHaveCount(0);
});
