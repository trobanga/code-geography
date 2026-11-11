import { describe, expect, it } from 'vitest';
import { fetchRepo, memoryCache, type Fetch, type Repo } from './forge';
import { github, parseRepo } from './github';

const resolve = (repo: Repo, net: { fetch: Fetch }) => github(net).resolve(repo);

const SHA = '0123456789abcdef0123456789abcdef01234567';

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

describe('parseRepo', () => {
  it('reads owner/repo without a ref', () => {
    expect(parseRepo('medizininformatik-initiative/fts-next')).toEqual({ ok: true, repo: { owner: 'medizininformatik-initiative', name: 'fts-next' } });
  });

  it('reads a ref after @', () => {
    expect(parseRepo(' a/b@v1.2 ')).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: 'v1.2' } });
  });

  it('reads GitHub URLs of a repository, a tree and a commit', () => {
    expect(parseRepo('https://github.com/a/b')).toEqual({ ok: true, repo: { owner: 'a', name: 'b' } });
    expect(parseRepo('https://github.com/a/b.git')).toEqual({ ok: true, repo: { owner: 'a', name: 'b' } });
    expect(parseRepo('github.com/a/b/')).toEqual({ ok: true, repo: { owner: 'a', name: 'b' } });
    expect(parseRepo('https://github.com/a/b/tree/main')).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: 'main' } });
  });

  it('reads a commit URL as the repository at that commit with the overlay of that commit', () => {
    expect(parseRepo('https://github.com/a/b/commit/88a95fa')).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: '88a95fa' }, overlay: { kind: 'commit' } });
  });

  it('reads owner/repo@A..B as the overlay of A to B, and A...B as from their merge base', () => {
    expect(parseRepo('a/b@v1..main')).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: 'main' }, overlay: { kind: 'range', base: 'v1', mergeBase: false } });
    expect(parseRepo('a/b@v1...feature/x')).toEqual({
      ok: true,
      repo: { owner: 'a', name: 'b', ref: 'feature/x' },
      overlay: { kind: 'range', base: 'v1', mergeBase: true },
    });
  });

  it('reads a compare URL with two or three dots, and a pull request URL', () => {
    expect(parseRepo('https://github.com/a/b/compare/v1...feature/x')).toEqual({
      ok: true,
      repo: { owner: 'a', name: 'b', ref: 'feature/x' },
      overlay: { kind: 'range', base: 'v1', mergeBase: true },
    });
    expect(parseRepo('github.com/a/b/compare/v1..v2')).toMatchObject({ overlay: { kind: 'range', base: 'v1', mergeBase: false } });
    expect(parseRepo('https://github.com/a/b/pull/42')).toEqual({ ok: true, repo: { owner: 'a', name: 'b' }, overlay: { kind: 'pull', n: 42 } });
    expect(parseRepo('https://github.com/a/b/pull/42/files')).toMatchObject({ overlay: { kind: 'pull', n: 42 } });
    for (const bad of ['https://github.com/a/b/compare/main', 'https://github.com/a/b/pull/x', 'https://github.com/a/b/pull/'])
      expect(parseRepo(bad), bad).toMatchObject({ ok: false });
  });

  it('takes the whole rest of a /tree/ URL as the ref and says so', () => {
    const p = parseRepo('https://github.com/a/b/tree/feature/x');
    expect(p).toMatchObject({ ok: true, repo: { owner: 'a', name: 'b', ref: 'feature/x' } });
    expect(p.ok && p.note).toMatch(/feature\/x.*whole branch name/);
    expect(parseRepo('https://github.com/a/b/tree/main')).not.toHaveProperty('note');
  });

  it('rejects anything else with a message that shows the accepted forms', () => {
    for (const bad of ['', 'a', 'a/', '/b', 'a/b/c', 'a b/c', 'a/b@', 'https://gitlab.com/a/b', 'https://github.com/a', 'https://github.com/a/b/blob/main/x.java', 'https://github.com/a/b/tree/'])
      expect(parseRepo(bad), bad).toEqual({ ok: false, error: expect.stringMatching(/owner\/repo/) });
  });
});

describe('resolve', () => {
  it('asks for the commit sha of the default branch in one call', async () => {
    const net = fakeFetch({ 'https://api.github.com/repos/a/b/commits/HEAD': () => new Response(SHA) });
    expect(await resolve({ owner: 'a', name: 'b' }, { fetch: net.fetch })).toEqual({ sha: SHA });
    expect(net.calls).toHaveLength(1);
    expect(net.calls[0].headers.Accept).toBe('application/vnd.github.sha');
  });

  it('takes a full commit sha as it is, without a call', async () => {
    const net = fakeFetch({});
    expect(await resolve({ owner: 'a', name: 'b', ref: SHA.toUpperCase() }, { fetch: net.fetch })).toEqual({ sha: SHA });
    expect(net.calls).toHaveLength(0);
  });

  it('asks for a branch with / in its name', async () => {
    const net = fakeFetch({ 'https://api.github.com/repos/a/b/commits/feature%2Fx': () => new Response(SHA + '\n') });
    expect(await resolve({ owner: 'a', name: 'b', ref: 'feature/x' }, { fetch: net.fetch })).toEqual({ sha: SHA });
  });

  it('says not found or private on 404', async () => {
    const net = fakeFetch({});
    await expect(resolve({ owner: 'a', name: 'b' }, { fetch: net.fetch })).rejects.toThrow('a/b: not found, or private');
  });

  it('says rate limit with the reset time on 403 and 429', async () => {
    const reset = 1_800_000_000;
    const at = new Date(reset * 1000).toLocaleTimeString();
    for (const status of [403, 429]) {
      const headers = { 'x-ratelimit-remaining': '0', 'x-ratelimit-reset': String(reset) };
      const net = fakeFetch({ 'https://api.github.com/repos/a/b/commits/HEAD': () => new Response('', { status, headers }) });
      await expect(resolve({ owner: 'a', name: 'b' }, { fetch: net.fetch })).rejects.toThrow(`GitHub rate limit reached (60 requests per hour without a token); try again after ${at}`);
    }
  });

  it('says that a ref is unknown on 422', async () => {
    const net = fakeFetch({ 'https://api.github.com/repos/a/b/commits/nope': () => new Response('', { status: 422 }) });
    await expect(resolve({ owner: 'a', name: 'b', ref: 'nope' }, { fetch: net.fetch })).rejects.toThrow('a/b: no branch, tag or commit "nope"');
  });

  it('says that GitHub cannot be reached when the request fails', async () => {
    const fetch: Fetch = () => Promise.reject(new TypeError('Failed to fetch'));
    await expect(resolve({ owner: 'a', name: 'b' }, { fetch })).rejects.toThrow('cannot reach api.github.com (network failure): Failed to fetch');
  });
});

const TREE_URL = `https://api.github.com/repos/a/b/git/trees/${SHA}?recursive=1`;
const json = (body: unknown) => () => new Response(JSON.stringify(body), { headers: { 'content-type': 'application/json' } });

describe('tree', () => {
  it('lists the blobs of a commit with their shas in one call', async () => {
    const net = fakeFetch({
      [TREE_URL]: json({
        truncated: false,
        tree: [
          { path: 'src', type: 'tree', sha: 't1' },
          { path: 'src/A.java', type: 'blob', sha: 'b1', size: 10 },
          { path: 'lib', type: 'commit', sha: 'c1' },
          { path: 'pom.xml', type: 'blob', sha: 'b2', size: 3 },
        ],
      }),
    });
    expect(await github({ fetch: net.fetch }).tree({ owner: 'a', name: 'b' }, SHA)).toEqual([
      { path: 'src/A.java', blobSha: 'b1' },
      { path: 'pom.xml', blobSha: 'b2' },
    ]);
    expect(net.calls).toHaveLength(1);
  });

  it('refuses a truncated tree rather than index a part of it', async () => {
    const net = fakeFetch({ [TREE_URL]: json({ truncated: true, tree: [] }) });
    await expect(github({ fetch: net.fetch }).tree({ owner: 'a', name: 'b' }, SHA)).rejects.toThrow('a/b: too large; GitHub truncated the file list');
  });

});

const raw = (path: string) => `https://raw.githubusercontent.com/a/b/${SHA}/${path}`;

describe('text', () => {
  it('fetches a file from the raw host at the commit, with each path segment encoded', async () => {
    const net = fakeFetch({ [raw('my%20dir/B%23.java')]: () => new Response('class B {}') });
    expect(await github({ fetch: net.fetch }).text({ owner: 'a', name: 'b' }, SHA, 'my dir/B#.java')).toBe('class B {}');
  });
});

describe('fetchRepo from GitHub', () => {
  const isInput = (p: string) => p.endsWith('.java') || p === 'pom.xml';
  const routes = () => ({
    'https://api.github.com/repos/a/b/commits/HEAD': () => new Response(SHA),
    [TREE_URL]: json({ truncated: false, tree: [{ path: 'README.md', type: 'blob', sha: 'r' }, { path: 'pom.xml', type: 'blob', sha: 'p' }, { path: 'A.java', type: 'blob', sha: 'j' }] }),
    [raw('pom.xml')]: () => new Response('<project/>'),
    [raw('A.java')]: () => new Response('class A {}'),
  });

  it('gives the inputs of the default branch with two API calls, and none of them on a second open by sha', async () => {
    const net = fakeFetch(routes());
    const cache = memoryCache();
    const phases: string[] = [];
    const got = await fetchRepo(github({ fetch: net.fetch }), { owner: 'a', name: 'b' }, cache, isInput, (p) => phases.push(p));
    expect(got).toEqual({ sha: SHA, root: 'b', paths: ['A.java', 'pom.xml'], texts: ['class A {}', '<project/>'] });
    expect(net.calls.filter((c) => c.url.startsWith('https://api.github.com'))).toHaveLength(2);
    expect(phases.at(-1)).toBe('fetched 2 / 2 files of a/b');

    const again = fakeFetch(routes());
    expect(await fetchRepo(github({ fetch: again.fetch }), { owner: 'a', name: 'b', ref: SHA }, cache, isInput)).toEqual(got);
    expect(again.calls).toHaveLength(0);
  });

});

const P1 = '1111111111111111111111111111111111111111';

describe('commits', () => {
  it('lists the last n commits reachable from a sha in one call: sha, parents, first line, author, date', async () => {
    const net = fakeFetch({
      [`https://api.github.com/repos/a/b/commits?sha=${SHA}&per_page=2`]: json([
        { sha: SHA, parents: [{ sha: P1 }, { sha: 'x' }], commit: { message: 'Merge it\n\nbody', author: { name: 'Ann', date: '2026-11-01T10:00:00Z' } } },
        { sha: P1, parents: [], commit: { message: 'First', author: { name: 'Bo', date: '2026-10-01T10:00:00Z' } } },
      ]),
    });
    expect(await github({ fetch: net.fetch }).commits({ owner: 'a', name: 'b' }, SHA, 2)).toEqual([
      { sha: SHA, parents: [P1, 'x'], title: 'Merge it', author: 'Ann', date: '2026-11-01T10:00:00Z' },
      { sha: P1, parents: [], title: 'First', author: 'Bo', date: '2026-10-01T10:00:00Z' },
    ]);
    expect(net.calls).toHaveLength(1);
  });

  it('lists a later page of the history', async () => {
    const net = fakeFetch({
      [`https://api.github.com/repos/a/b/commits?sha=${SHA}&per_page=2&page=3`]: json([
        { sha: P1, parents: [], commit: { message: 'First', author: { name: 'Bo', date: '2026-10-01T10:00:00Z' } } },
      ]),
    });
    expect((await github({ fetch: net.fetch }).commits({ owner: 'a', name: 'b' }, SHA, 2, 3)).map((c) => c.sha)).toEqual([P1]);
  });
});

const P2 = '2222222222222222222222222222222222222222';

describe('mergeBase and pull', () => {
  it('finds the merge base of two refs in one call', async () => {
    const net = fakeFetch({
      'https://api.github.com/repos/a/b/compare/main...feature%2Fx?per_page=1': json({ merge_base_commit: { sha: P1 } }),
    });
    expect(await github({ fetch: net.fetch }).mergeBase({ owner: 'a', name: 'b' }, 'main', 'feature/x')).toBe(P1);
    expect(net.calls).toHaveLength(1);
  });

  it('gives the head of a pull request and its merge base with its base branch, in two calls', async () => {
    const net = fakeFetch({
      'https://api.github.com/repos/a/b/pulls/7': json({ head: { sha: SHA }, base: { sha: P2 } }),
      [`https://api.github.com/repos/a/b/compare/${P2}...${SHA}?per_page=1`]: json({ merge_base_commit: { sha: P1 } }),
    });
    expect(await github({ fetch: net.fetch }).pull({ owner: 'a', name: 'b' }, 7)).toEqual({ base: P1, head: SHA });
    expect(net.calls).toHaveLength(2);
  });

  it('says that a pull request does not exist on 404', async () => {
    const net = fakeFetch({});
    await expect(github({ fetch: net.fetch }).pull({ owner: 'a', name: 'b' }, 7)).rejects.toThrow('a/b: no pull request #7');
  });

  it('says that a compare failed on 404: an unknown ref or no common history', async () => {
    const net = fakeFetch({});
    await expect(github({ fetch: net.fetch }).mergeBase({ owner: 'a', name: 'b' }, 'main', 'nope')).rejects.toThrow(
      'a/b: cannot compare main...nope: an unknown branch, tag or commit, or no common history',
    );
  });
});
