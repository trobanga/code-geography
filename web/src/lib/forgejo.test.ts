import { describe, expect, it } from 'vitest';
import type { Fetch } from './forge';
import { forgejo } from './forgejo';

// Answers of Codeberg (Forgejo 17, Gitea API 1.22), recorded 2026-11-11 on mergiraf/mergiraf, shortened.
const ORIGIN = 'https://codeberg.org';
const API = `${ORIGIN}/api/v1/repos/mergiraf/mergiraf`;
const SHA = 'c38402e9e663c7e38b21f0082fd1fdcc4d97af59';
const PARENT = 'eca5febcf9735959564d62a039f508554304566c';
const repo = { owner: 'mergiraf', name: 'mergiraf' };
const LEAN = 'stat=false&verification=false&files=false';

/** GET /git/commits/{ref}: also a branch, a tag or a short sha. */
const GIT_COMMIT = {
  url: `${API}/git/commits/${SHA}`,
  sha: SHA,
  created: '2026-11-08T22:59:37+01:00',
  commit: { message: 'chore: switch Javascript parser to maintained repo\n', author: { name: 'Antonin Delpeuch', email: 'a@d.eu', date: '2026-11-08T22:59:37+01:00' } },
  parents: [{ url: `${API}/git/commits/${PARENT}`, sha: PARENT, created: '2026-11-04T16:35:06+01:00' }],
};

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json;charset=utf-8' } });

/** A fake network: answers by URL, records each request. */
function fakeFetch(routes: Record<string, () => Response>) {
  const calls: { url: string; headers: Record<string, string> }[] = [];
  const fetch: Fetch = async (url, init) => {
    calls.push({ url, headers: { ...init?.headers } });
    const r = routes[url];
    if (!r) return json({ message: "The target couldn't be found.", url: `${ORIGIN}/api/swagger`, errors: [] }, 404);
    return r();
  };
  return { fetch, calls };
}

describe('forgejo resolve', () => {
  it('asks for the commit of a branch in one call', async () => {
    const net = fakeFetch({ [`${API}/git/commits/main?${LEAN}`]: () => json(GIT_COMMIT) });
    expect(await forgejo(net, ORIGIN).resolve({ ...repo, ref: 'main' })).toEqual({ sha: SHA });
    expect(net.calls.length).toBe(1);
  });
});

describe('forgejo resolve, more', () => {
  it('takes a full sha without a call, asks for HEAD without a ref, and encodes a ref with /', async () => {
    const net = fakeFetch({
      [`${API}/git/commits/HEAD?${LEAN}`]: () => json(GIT_COMMIT),
      [`${API}/git/commits/feature%2Fx?${LEAN}`]: () => json({ ...GIT_COMMIT, sha: PARENT }),
    });
    const f = forgejo(net, ORIGIN);
    expect(await f.resolve({ ...repo, ref: SHA.toUpperCase() })).toEqual({ sha: SHA });
    expect(net.calls.length).toBe(0);
    expect(await f.resolve(repo)).toEqual({ sha: SHA });
    expect(await f.resolve({ ...repo, ref: 'feature/x' })).toEqual({ sha: PARENT });
  });
});

/** GET /git/trees/{sha}?recursive=true: 1000 entries per page at most; `truncated` is still true on the last page with entries. */
const entry = (path: string, type: 'blob' | 'tree', sha: string) => ({ path, mode: type === 'tree' ? '040000' : '100644', type, size: 1, sha, url: `${API}/git/blobs/${sha}` });
const treePage = (page: number, total: number, tree: unknown[], truncated = true) => ({ sha: SHA, url: `${API}/git/trees/${SHA}`, tree, truncated, page, total_count: total });

describe('forgejo tree', () => {
  it('reads all pages until it has total_count entries, and keeps the blobs', async () => {
    const net = fakeFetch({
      [`${API}/git/trees/${SHA}?recursive=true&page=1`]: () =>
        json(treePage(1, 4, [entry('src', 'tree', 't1'), entry('src/main.rs', 'blob', 'b1')])),
      [`${API}/git/trees/${SHA}?recursive=true&page=2`]: () => json(treePage(2, 4, [entry('Cargo.toml', 'blob', 'b2'), entry('README.md', 'blob', 'b3')])),
    });
    expect(await forgejo(net, ORIGIN).tree(repo, SHA)).toEqual([
      { path: 'src/main.rs', blobSha: 'b1' },
      { path: 'Cargo.toml', blobSha: 'b2' },
      { path: 'README.md', blobSha: 'b3' },
    ]);
    expect(net.calls.length).toBe(2);
  });
});

describe('forgejo tree, partial', () => {
  it('refuses a tree whose pages end before total_count, or give more than it', async () => {
    const short = fakeFetch({
      [`${API}/git/trees/${SHA}?recursive=true&page=1`]: () => json(treePage(1, 3, [entry('a.rs', 'blob', 'b1')])),
      [`${API}/git/trees/${SHA}?recursive=true&page=2`]: () => json(treePage(2, 3, [], false)),
    });
    await expect(forgejo(short, ORIGIN).tree(repo, SHA)).rejects.toThrow(/mergiraf\/mergiraf: .*1 of 3 entries/);
    const more = fakeFetch({
      [`${API}/git/trees/${SHA}?recursive=true&page=1`]: () => json(treePage(1, 1, [entry('a.rs', 'blob', 'b1'), entry('b.rs', 'blob', 'b2')])),
    });
    await expect(forgejo(more, ORIGIN).tree(repo, SHA)).rejects.toThrow(/2 of 1 entries/);
  });
});

describe('forgejo text', () => {
  it('fetches a file from the raw API at the commit, each path segment encoded, as UTF-8', async () => {
    const net = fakeFetch({
      [`${API}/raw/src/a%20b/%C3%BCber.rs?ref=${SHA}`]: () =>
        new Response(new TextEncoder().encode('// über\n'), { headers: { 'content-type': 'text/plain; charset=utf-8' } }),
    });
    expect(await forgejo(net, ORIGIN).text(repo, SHA, 'src/a b/über.rs')).toBe('// über\n');
  });
});

describe('forgejo commits and pull', () => {
  it('lists the last n commits reachable from a sha in one call: sha, parents, first line, author, date', async () => {
    const c = (sha: string, parent: string, message: string) => ({
      url: `${API}/git/commits/${sha}`,
      sha,
      commit: { message, author: { name: 'Ben Boeckel', email: 'm@n.org', date: '2026-11-04T16:35:06+01:00' } },
      parents: [{ url: `${API}/git/commits/${parent}`, sha: parent }],
    });
    const net = fakeFetch({ [`${API}/commits?sha=${SHA}&limit=2&${LEAN}`]: () => json([c(SHA, PARENT, 'chore: switch\n\nbody'), c(PARENT, 'p0', 'cargo-update (#857)')]) });
    expect(await forgejo(net, ORIGIN).commits(repo, SHA, 2)).toEqual([
      { sha: SHA, parents: [PARENT], title: 'chore: switch', author: 'Ben Boeckel', date: '2026-11-04T16:35:06+01:00', message: 'chore: switch\n\nbody' },
      { sha: PARENT, parents: ['p0'], title: 'cargo-update (#857)', author: 'Ben Boeckel', date: '2026-11-04T16:35:06+01:00', message: 'cargo-update (#857)' },
    ]);
  });

  it('lists a later page of the history', async () => {
    const one = { sha: PARENT, commit: { message: 'm', author: null }, parents: [] };
    const net = fakeFetch({ [`${API}/commits?sha=${SHA}&limit=2&${LEAN}&page=3`]: () => json([one]) });
    expect((await forgejo(net, ORIGIN).commits(repo, SHA, 2, 3)).map((c) => c.sha)).toEqual([PARENT]);
  });

  it('gives the head of a pull request and the merge base that it carries, in one call', async () => {
    const net = fakeFetch({
      [`${API}/pulls/857`]: () =>
        json({
          number: 857,
          merge_base: '1e848e491951b94e233c9ca52102b7b7e63bde39',
          head: { ref: 'refs/pull/857/head', sha: '72fd895e0aa509988728a401ea22fdcd934cdbde' },
          base: { ref: 'main', sha: SHA },
        }),
    });
    expect(await forgejo(net, ORIGIN).pull(repo, 857)).toEqual({ base: '1e848e491951b94e233c9ca52102b7b7e63bde39', head: '72fd895e0aa509988728a401ea22fdcd934cdbde' });
    expect(net.calls.length).toBe(1);
  });
});

describe('forgejo branches', () => {
  const b = (name: string, id: string) => ({ name, commit: { id, message: 'm', url: `${ORIGIN}/mergiraf/mergiraf/commit/${id}` }, protected: false });
  /** A page of branches; `total` is the `X-Total-Count` header, which Forgejo exposes to a browser. */
  const page = (names: string[], total?: number) => () =>
    new Response(JSON.stringify(names.map((n) => b(n, SHA))), { headers: total === undefined ? {} : { 'x-total-count': String(total) } });

  it('lists the branches with the id of their head commit, with the token of the origin; one call when the first page holds all', async () => {
    const net = fakeFetch({
      [`${API}/branches?limit=100`]: () => new Response(JSON.stringify([b('main', SHA), b('feature/x', PARENT)]), { headers: { 'x-total-count': '2' } }),
    });
    expect(await forgejo(net, ORIGIN, 'secret').branches(repo)).toEqual([
      { name: 'main', sha: SHA },
      { name: 'feature/x', sha: PARENT },
    ]);
    expect(net.calls).toEqual([{ url: `${API}/branches?limit=100`, headers: { Authorization: 'token secret' } }]);
  });

  it('lists all branches: further pages until the announced total, also when the host gives smaller pages than asked for', async () => {
    const net = fakeFetch({
      [`${API}/branches?limit=100`]: page(['a', 'b'], 3),
      [`${API}/branches?limit=100&page=2`]: page(['c'], 3),
    });
    expect((await forgejo(net, ORIGIN).branches(repo)).map((x) => x.name)).toEqual(['a', 'b', 'c']);
    expect(net.calls).toHaveLength(2);
  });

  it('without an announced total, or with a total that the pages do not reach, asks until a page is empty', async () => {
    const silent = fakeFetch({
      [`${API}/branches?limit=100`]: page(['a', 'b']),
      [`${API}/branches?limit=100&page=2`]: page(['c']),
      [`${API}/branches?limit=100&page=3`]: page([]),
    });
    expect((await forgejo(silent, ORIGIN).branches(repo)).map((x) => x.name)).toEqual(['a', 'b', 'c']);
    // A branch was deleted between two calls.
    const short = fakeFetch({ [`${API}/branches?limit=100`]: page(['a', 'b'], 3), [`${API}/branches?limit=100&page=2`]: page([], 2) });
    expect((await forgejo(short, ORIGIN).branches(repo)).map((x) => x.name)).toEqual(['a', 'b']);
    expect(short.calls).toHaveLength(2);
  });
});

describe('forgejo mergeBase', () => {
  const MB = '1e848e491951b94e233c9ca52102b7b7e63bde39';
  const c = (sha: string, ...parents: string[]) => ({ sha, parents: parents.map((p) => ({ sha: p })) });
  const cmp = (a: string, b: string) => `${API}/compare/${a}...${b}?files=false&verification=false`;

  it('takes the one parent outside the compared commits: Forgejo lists the commits from the merge base to b, without it', async () => {
    const net = fakeFetch({ [cmp('main', 'h3')]: () => json({ commits: [c('h3', 'h2'), c('h2', 'h1'), c('h1', MB)], total_commits: 3, files: [] }) });
    expect(await forgejo(net, ORIGIN).mergeBase(repo, 'main', 'h3')).toBe(MB);
    expect(net.calls.length).toBe(1);
  });

  it('resolves b when no commit is listed: b is then an ancestor of a', async () => {
    const net = fakeFetch({
      [cmp('main', 'v1')]: () => json({ commits: [], total_commits: 0, files: [] }),
      [`${API}/git/commits/v1?${LEAN}`]: () => json({ ...GIT_COMMIT, sha: MB }),
    });
    expect(await forgejo(net, ORIGIN).mergeBase(repo, 'main', 'v1')).toBe(MB);
  });

  it('picks among several outside parents the one that has the others as ancestors, one compare each', async () => {
    // Branch f from m1, main merged into f at m2: the outside parents are m1 and m2, and m1 is an ancestor of m2.
    const net = fakeFetch({
      [cmp('main', 'f')]: () => json({ commits: [c('f', 'f2', 'm2'), c('f2', 'f1'), c('f1', 'm1')], total_commits: 3, files: [] }),
      // m2...m1 lists nothing: m1 is an ancestor of m2.
      [cmp('m2', 'm1')]: () => json({ commits: [], total_commits: 0, files: [] }),
    });
    expect(await forgejo(net, ORIGIN).mergeBase(repo, 'main', 'f')).toBe('m2');
  });

  it('refuses a compare that lists fewer commits than it counts', async () => {
    const net = fakeFetch({ [cmp('main', 'h3')]: () => json({ commits: [c('h3', 'h2')], total_commits: 3, files: [] }) });
    await expect(forgejo(net, ORIGIN).mergeBase(repo, 'main', 'h3')).rejects.toThrow(/1 of 3 commits/);
  });
});

describe('forgejo errors', () => {
  const at = `${API}/git/commits/main?${LEAN}`;
  const fail = (status: number, token?: string, body: unknown = { message: `access token does not exist [sha: ${token}]` }) =>
    forgejo(fakeFetch({ [at]: () => json(body, status) }), ORIGIN, token)
      .resolve({ ...repo, ref: 'main' })
      .then(
        () => '',
        (e: Error) => e.message,
      );

  it('says that the token is wrong or expired on 401, and never repeats the answer, which may echo the token', async () => {
    const m = await fail(401, 'secret-token');
    expect(m).toMatch(/codeberg\.org: the token is wrong or expired/);
    expect(m).not.toContain('secret-token');
  });

  it('says on 403 that the token lacks the scope read:repository', async () => {
    expect(await fail(403, 'secret-token')).toMatch(/read:repository/);
  });

  it('says on 404 without a token: not found, or private: give a token; with a token: not found or no access', async () => {
    expect(await fail(404)).toMatch(/mergiraf\/mergiraf: no branch, tag or commit "main", or the repository is not found or private: give a token/);
    expect(await fail(404, 'secret-token')).toMatch(/not found, or the token of codeberg\.org has no access to it/);
  });

  it('says too many requests on 429', async () => {
    expect(await fail(429)).toMatch(/codeberg\.org: too many requests/);
  });

  it('names CORS as the probable cause when the request fails, and what the admin must set', async () => {
    const net = {
      fetch: async () => {
        throw new TypeError('NetworkError when attempting to fetch resource.');
      },
    };
    const m = await forgejo(net, ORIGIN, 'secret-token')
      .resolve(repo)
      .then(
        () => '',
        (e: Error) => e.message,
      );
    expect(m).toMatch(/cannot reach codeberg\.org/);
    expect(m).toMatch(/\[cors\] ENABLED = true/);
    expect(m).toMatch(/ALLOW_DOMAIN/);
    expect(m).not.toContain('secret-token');
  });
});

