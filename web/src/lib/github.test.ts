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
    expect(parseRepo('https://github.com/a/b/commit/88a95fa')).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: '88a95fa' } });
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
