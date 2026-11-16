import { describe, expect, it } from 'vitest';
import type { Fetch } from './forge';
import { github } from './github';

const SHA = '0123456789abcdef0123456789abcdef01234567';
const repo = { owner: 'a', name: 'b' };
const API = 'https://api.github.com/repos/a/b';

/** A fake network: answers by URL, records each request. */
function fakeFetch(routes: Record<string, () => Response>) {
  const calls: { url: string; headers: Record<string, string> }[] = [];
  const fetch: Fetch = async (url, init) => {
    calls.push({ url, headers: { ...(init?.headers as Record<string, string>) } });
    const r = routes[url];
    if (!r) return new Response('no route', { status: 404 });
    return r();
  };
  return { fetch, calls };
}
const json = (body: unknown) => () => new Response(JSON.stringify(body));

describe('github between', () => {
  const P = 'f'.repeat(40);
  const commit = (sha: string, parent: string, message: string) => ({
    sha,
    parents: [{ sha: parent }],
    commit: { message, author: { name: 'Ann', date: '2026-11-01T10:00:00Z' } },
  });

  it('lists the commits of a range newest first, in one call', async () => {
    const net = fakeFetch({
      [`${API}/compare/main...feat?per_page=100`]: json({
        total_commits: 2,
        commits: [commit('c1', P, 'first\n\nbody one'), commit('c2', 'c1', 'second')],
      }),
    });
    const r = await github(net).between(repo, 'main', 'feat');
    expect(r.total).toBe(2);
    expect(r.commits).toEqual([
      { sha: 'c2', parents: ['c1'], title: 'second', message: 'second', author: 'Ann', date: '2026-11-01T10:00:00Z' },
      { sha: 'c1', parents: [P], title: 'first', message: 'first\n\nbody one', author: 'Ann', date: '2026-11-01T10:00:00Z' },
    ]);
    expect(net.calls.length).toBe(1);
  });

  it('lists the newest commits of a range longer than 100, with the head, in two calls', async () => {
    const page = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => commit(`c${from + i}`, P, `m${from + i}`));
    const net = fakeFetch({
      [`${API}/compare/main...feat?per_page=100`]: json({ total_commits: 300, commits: page(0, 100) }),
      // Pages of the compare are of equal size, oldest first: the last one holds the head.
      [`${API}/compare/main...feat?per_page=100&page=3`]: json({ total_commits: 300, commits: page(200, 300) }),
    });
    const r = await github(net).between(repo, 'main', 'feat');
    expect([r.total, r.commits.length, r.commits[0].sha, r.commits[99].sha]).toEqual([300, 100, 'c299', 'c200']);
    expect(net.calls.length).toBe(2);
  });

  it('picks the page size whose last page holds the most commits', async () => {
    const page = (from: number, to: number) => Array.from({ length: to - from }, (_, i) => commit(`c${from + i}`, P, `m${from + i}`));
    const net = fakeFetch({
      [`${API}/compare/main...feat?per_page=100`]: json({ total_commits: 101, commits: page(0, 100) }),
      // 101 = 51 + 50: no page size up to 100 gives a last page of more than 50.
      [`${API}/compare/main...feat?per_page=51&page=2`]: json({ total_commits: 101, commits: page(51, 101) }),
    });
    const r = await github(net).between(repo, 'main', 'feat');
    expect([r.total, r.commits.length, r.commits[0].sha]).toEqual([101, 50, 'c100']);
    expect(net.calls.length).toBe(2);
  });

  it('gives the head alone for a root commit', async () => {
    const net = fakeFetch({ [`${API}/commits?sha=${SHA}&per_page=1`]: json([{ ...commit(SHA, P, 'root\n\nmore'), parents: [] }]) });
    const r = await github(net).between(repo, null, SHA);
    expect(r.total).toBe(1);
    expect(r.commits.map((c) => [c.sha, c.title, c.message])).toEqual([[SHA, 'root', 'root\n\nmore']]);
  });
});

describe('github pullsOf', () => {
  const URL = `${API}/commits/${SHA}/pulls`;

  it('maps the pull requests of a commit in one call', async () => {
    const net = fakeFetch({
      [URL]: json([
        { number: 7, title: 'Fix it', state: 'closed', merged_at: '2026-11-01T10:00:00Z', html_url: 'https://github.com/a/b/pull/7', body: 'Closes #3' },
        { number: 8, title: 'Wip', state: 'open', merged_at: null, html_url: 'https://github.com/a/b/pull/8', body: null },
        { number: 9, title: 'No', state: 'closed', merged_at: null, html_url: 'https://github.com/a/b/pull/9', body: 'x' },
      ]),
    });
    expect(await github(net).pullsOf(repo, SHA)).toEqual([
      { number: 7, title: 'Fix it', state: 'merged', url: 'https://github.com/a/b/pull/7', body: 'Closes #3' },
      { number: 8, title: 'Wip', state: 'open', url: 'https://github.com/a/b/pull/8', body: '' },
      { number: 9, title: 'No', state: 'closed', url: 'https://github.com/a/b/pull/9', body: 'x' },
    ]);
    expect(net.calls.map((c) => c.url)).toEqual([URL]);
  });

  it('rejects with the rate limit message', async () => {
    const net = fakeFetch({ [URL]: () => new Response('', { status: 403, headers: { 'x-ratelimit-remaining': '0' } }) });
    await expect(github(net).pullsOf(repo, SHA)).rejects.toThrow('GitHub rate limit reached');
  });
});
