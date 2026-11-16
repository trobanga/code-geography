import { describe, expect, it } from 'vitest';
import type { Fetch } from './forge';
import { forgejo } from './forgejo';

const ORIGIN = 'https://codeberg.org';
const API = `${ORIGIN}/api/v1/repos/mergiraf/mergiraf`;
const SHA = 'c38402e9e663c7e38b21f0082fd1fdcc4d97af59';
const repo = { owner: 'mergiraf', name: 'mergiraf' };

const json = (body: unknown, status = 200) => () => new Response(JSON.stringify(body), { status });

/** A fake network: answers by URL, records each request. */
function fakeFetch(routes: Record<string, () => Response>) {
  const calls: { url: string; headers: Record<string, string> }[] = [];
  const fetch: Fetch = async (url, init) => {
    calls.push({ url, headers: { ...init?.headers } });
    const r = routes[url];
    return r ? r() : json({ message: "The target couldn't be found." }, 404)();
  };
  return { fetch, calls };
}

describe('forgejo between', () => {
  const commit = (sha: string, parent: string, message: string) => ({
    sha,
    parents: [{ sha: parent }],
    commit: { message, author: { name: 'Ann', date: '2026-11-01T10:00:00+01:00' } },
  });

  it('keeps the order of the compare, and a partial list with the total', async () => {
    const net = fakeFetch({
      [`${API}/compare/main...feat?files=false&verification=false`]: json({
        total_commits: 5,
        commits: [commit('c2', 'c1', 'second\n\nmore'), commit('c1', 'c0', 'first')],
      }),
    });
    const r = await forgejo(net, ORIGIN).between(repo, 'main', 'feat');
    expect(r.total).toBe(5);
    expect(r.commits).toEqual([
      { sha: 'c2', parents: ['c1'], title: 'second', message: 'second\n\nmore', author: 'Ann', date: '2026-11-01T10:00:00+01:00' },
      { sha: 'c1', parents: ['c0'], title: 'first', message: 'first', author: 'Ann', date: '2026-11-01T10:00:00+01:00' },
    ]);
    expect(net.calls.length).toBe(1);
  });

  it('gives the head alone for a root commit', async () => {
    const net = fakeFetch({
      [`${API}/commits?sha=${SHA}&limit=1&stat=false&verification=false&files=false`]: json([{ ...commit(SHA, 'x', 'root\n\nbody'), parents: null }]),
    });
    const r = await forgejo(net, ORIGIN).between(repo, null, SHA);
    expect(r.total).toBe(1);
    expect(r.commits.map((c) => [c.sha, c.parents, c.message])).toEqual([[SHA, [], 'root\n\nbody']]);
  });
});

describe('forgejo pullsOf', () => {
  const URL = `${API}/commits/${SHA}/pull`;

  it('gives the one pull request of a merge commit as a list', async () => {
    const net = fakeFetch({
      [URL]: json({ number: 12, title: 'Add it', state: 'closed', merged: true, html_url: `${ORIGIN}/mergiraf/mergiraf/pulls/12`, body: 'Fixes #4' }),
    });
    expect(await forgejo(net, ORIGIN).pullsOf(repo, SHA)).toEqual([
      { number: 12, title: 'Add it', state: 'merged', url: `${ORIGIN}/mergiraf/mergiraf/pulls/12`, body: 'Fixes #4' },
    ]);
    expect(net.calls.map((c) => c.url)).toEqual([URL]);
  });

  it('gives an open or closed state when not merged, and an empty body for none', async () => {
    const net = fakeFetch({ [URL]: json({ number: 13, title: 'T', state: 'open', merged: false, html_url: 'u', body: null }) });
    expect((await forgejo(net, ORIGIN).pullsOf(repo, SHA))[0]).toMatchObject({ state: 'open', body: '' });
  });

  it('gives an empty list for 404: no pull request merged with this commit', async () => {
    const net = fakeFetch({});
    expect(await forgejo(net, ORIGIN).pullsOf(repo, SHA)).toEqual([]);
  });

  it('rejects for other failures', async () => {
    const net = fakeFetch({ [URL]: json({}, 403) });
    await expect(forgejo(net, ORIGIN).pullsOf(repo, SHA)).rejects.toThrow('refused to read mergiraf/mergiraf (HTTP 403)');
  });

  it('links the pull request to its page built from the number, not to the URL of the answer', async () => {
    const net = fakeFetch({ [URL]: json({ number: 13, title: 'T', state: 'open', merged: false, html_url: 'javascript:alert(1)', body: null }) });
    expect((await forgejo(net, ORIGIN).pullsOf(repo, SHA))[0].url).toBe(`${ORIGIN}/mergiraf/mergiraf/pulls/13`);
  });
});

describe('forgejo links', () => {
  it('builds the pages of a commit, a pull request and an issue of another repository on the same host', () => {
    const l = forgejo(fakeFetch({}), ORIGIN).links(repo);
    expect([l.commit(SHA), l.pull(7), l.issue('x', 'y', 3)]).toEqual([`${ORIGIN}/mergiraf/mergiraf/commit/${SHA}`, `${ORIGIN}/mergiraf/mergiraf/pulls/7`, `${ORIGIN}/x/y/issues/3`]);
  });

  it('reads the issue and pull request URLs of its own host, and no other', () => {
    const l = forgejo(fakeFetch({}), ORIGIN).links(repo);
    expect(l.reference(`${ORIGIN}/x/y/issues/3`)).toEqual({ owner: 'x', name: 'y', n: 3 });
    expect(l.reference(`${ORIGIN}/a/b/pulls/12`)).toEqual({ owner: 'a', name: 'b', n: 12 });
    expect(l.reference(`${ORIGIN}/a/b/pull/12`)).toBeNull();
    expect(l.reference('https://github.com/a/b/issues/3')).toBeNull();
    expect(l.reference('https://codeberg.org.evil.example/a/b/issues/3')).toBeNull();
  });
});
