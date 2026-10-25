import { expect, createNode, type APIRequestContext } from './fixtures';

/**
 * A small trove that looks like real data (#135), for specs whose feature
 * depends on content: search, ranking, pickers. Ad-hoc fixtures with empty
 * bodies and unique words hid the v2.33.0 picker bug, which only appears
 * when other nodes *mention* a title in their bodies and titles share
 * prefixes.
 *
 * Every title carries `stamp`, so parallel retries never collide, and
 * `cleanup()` removes everything the seed created.
 */
export type SeededTrove = {
  /** A distinctive word used as a title and mentioned in other bodies. */
  word: string;
  /** Node titled exactly `word`. */
  exact: { id: string; title: string };
  /** Node titled `${word} roadmap` (a prefix match). */
  prefixed: { id: string; title: string };
  /** Nodes whose BODY mentions `word`, sorted before it by title. */
  mentions: { id: string; title: string }[];
  cleanup: () => Promise<void>;
};

export async function seedTrove(
  request: APIRequestContext,
  stamp: number,
  mentionCount = 9,
): Promise<SeededTrove> {
  const word = `Zorbix${stamp}`;
  const ids: string[] = [];
  const make = async (title: string, body = '', node_type = 'article') => {
    const node = await createNode(request, title, { body, node_type });
    ids.push(node.id);
    return { id: node.id as string, title };
  };
  const mentions = [];
  for (let i = 0; i < mentionCount; i++) {
    mentions.push(
      await make(
        `aa meeting ${i} ${stamp}`,
        `Met with ${word} about the roadmap.\n\nSee also [[${word} roadmap]].`,
      ),
    );
  }
  const exact = await make(word, `# ${word}\n\nThe company.`, 'area');
  const prefixed = await make(`${word} roadmap`, `Plans for ${word}.`, 'project');
  const task = await request.post(`/api/nodes/${prefixed.id}/tasks`, {
    data: { title: `Draft the ${word} plan` },
  });
  expect(task.ok()).toBeTruthy();
  const note = await request.post(`/api/nodes/${exact.id}/notes`, {
    data: { body: `Call notes: ${word} wants a demo.`, color: 'default' },
  });
  expect(note.ok()).toBeTruthy();
  return {
    word,
    exact,
    prefixed,
    mentions,
    cleanup: async () => {
      for (const id of ids) await request.delete(`/api/nodes/${id}`);
    },
  };
}
