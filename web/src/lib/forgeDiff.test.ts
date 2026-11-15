import { describe, expect, it } from 'vitest';
import { memoryCache, type Entry, type Forge } from './forge';
import { byUpdateBot, cachedCommits, commitRows, listedCommits, overlayNames, fetchOverlay, resolveRange, treeChanges } from './forgeDiff';

const e = (path: string, blobSha: string) => ({ path, blobSha });

describe('treeChanges', () => {
  it('compares two trees by blob sha: only in head added, only in base removed, other sha modified; sorted by path', () => {
    const base = [e('z.java', '1'), e('README.md', '2'), e('a/Same.java', '3'), e('gone.txt', '4')];
    const head = [e('a/Same.java', '3'), e('README.md', '5'), e('new/N.java', '6'), e('z.java', '1')];
    expect(treeChanges(base, head)).toEqual([
      { path: 'README.md', change: 'modified' },
      { path: 'gone.txt', change: 'removed' },
      { path: 'new/N.java', change: 'added' },
    ]);
  });
});

const BASE = 'b'.repeat(40);
const HEAD = 'h'.repeat(40);
const REPO = { owner: 'o', name: 'r' };
const isInput = (p: string) => p.endsWith('.java') || p === 'pom.xml';

/** A forge with one file set per commit; the blob sha is the text, so equal texts share a blob. */
function twoCommits(commits: Record<string, Record<string, string>>) {
  const calls: string[] = [];
  const forge: Forge = {
    host: 'example.org',
    name: 'Example',
    async tree(_repo, sha) {
      calls.push(`tree ${sha[0]}`);
      return Object.entries(commits[sha]).map(([path, text]): Entry => ({ path, blobSha: `blob:${text}` }));
    },
    async text(_repo, sha, path) {
      calls.push(`text ${sha[0]} ${path}`);
      return commits[sha][path];
    },
    ...history(calls),
  };
  return { forge, calls };
}

/** The history part of a fake forge: HEAD has parent BASE, ROOT has none; refs resolve to `<ref>-sha`. */
function history(calls: string[]): Pick<Forge, 'resolve' | 'commits' | 'mergeBase' | 'pull' | 'branches'> {
  const commit = (sha: string, parents: string[]) => ({ sha, parents, title: `t ${sha[0]}`, author: 'A', date: '2026-11-01T00:00:00Z' });
  return {
    async resolve(repo) {
      calls.push(`resolve ${repo.ref}`);
      return { sha: `${repo.ref}-sha` };
    },
    async commits(_repo, sha, n) {
      calls.push(`commits ${sha} ${n}`);
      if (sha === 'root') return [commit('r'.repeat(40), [])];
      return [commit(HEAD, [BASE, 'other']), commit(BASE, [])].slice(0, n);
    },
    async mergeBase(_repo, a, b) {
      calls.push(`mergeBase ${a} ${b}`);
      return `mb-of-${a}-${b}`;
    },
    async pull(_repo, n) {
      calls.push(`pull ${n}`);
      return { base: 'pull-base', head: 'pull-head' };
    },
    branches: async () => [],
  };
}

describe('fetchOverlay', () => {
  it('fetches for the base only the files that the cache lacks, and lists every changed file', async () => {
    const { forge, calls } = twoCommits({
      [BASE]: { 'pom.xml': '<p/>', 'A.java': 'class A {}', 'Gone.java': 'class Gone {}', 'README.md': 'old' },
      [HEAD]: { 'pom.xml': '<p/>', 'A.java': 'class A { int x; }', 'New.java': 'class New {}', 'README.md': 'new' },
    });
    const got = await fetchOverlay(forge, REPO, { base: BASE, head: HEAD }, memoryCache(), isInput);
    expect(got.root).toBe('r');
    expect(got.head).toEqual({ paths: ['A.java', 'New.java', 'pom.xml'], texts: ['class A { int x; }', 'class New {}', '<p/>'] });
    expect(got.base).toEqual({ paths: ['A.java', 'Gone.java', 'pom.xml'], texts: ['class A {}', 'class Gone {}', '<p/>'] });
    expect(got.changed).toEqual([
      { path: 'A.java', change: 'modified' },
      { path: 'Gone.java', change: 'removed' },
      { path: 'New.java', change: 'added' },
      { path: 'README.md', change: 'modified' },
    ]);
    // pom.xml is unchanged: fetched once, for the head; README.md is no input: never fetched.
    expect(calls.filter((c) => c.startsWith('text')).sort()).toEqual([
      'text b A.java',
      'text b Gone.java',
      'text h A.java',
      'text h New.java',
      'text h pom.xml',
    ]);
  });

  it('compares a root commit with nothing: no base tree, every file added', async () => {
    const { forge, calls } = twoCommits({ [HEAD]: { 'A.java': 'class A {}', 'README.md': 'x' } });
    const got = await fetchOverlay(forge, REPO, { base: null, head: HEAD }, memoryCache(), isInput);
    expect(got.base).toEqual({ paths: [], texts: [] });
    expect(got.changed).toEqual([
      { path: 'A.java', change: 'added' },
      { path: 'README.md', change: 'added' },
    ]);
    expect(calls.filter((c) => c.startsWith('tree'))).toEqual(['tree h']);
  });
});

describe('resolveRange', () => {
  it('takes a commit and its first parent from one history call; a root commit has no base', async () => {
    const calls: string[] = [];
    const forge = { host: 'h', name: 'h', tree: async () => [], text: async () => '', ...history(calls) };
    expect(await resolveRange(forge, { ...REPO, ref: 'abc1234' }, { kind: 'commit' })).toEqual({ base: BASE, head: HEAD });
    expect(await resolveRange(forge, { ...REPO, ref: 'root' }, { kind: 'commit' })).toEqual({ base: null, head: 'r'.repeat(40) });
    expect(calls).toEqual(['commits abc1234 1', 'commits root 1']);
  });

  it('resolves both ends of A..B; for A...B the head, then the merge base of A and it', async () => {
    const calls: string[] = [];
    const forge = { host: 'h', name: 'h', tree: async () => [], text: async () => '', ...history(calls) };
    expect(await resolveRange(forge, { ...REPO, ref: 'v2' }, { kind: 'range', base: 'v1', mergeBase: false })).toEqual({ base: 'v1-sha', head: 'v2-sha' });
    expect(await resolveRange(forge, { ...REPO, ref: 'f' }, { kind: 'range', base: 'main', mergeBase: true })).toEqual({ base: 'mb-of-main-f-sha', head: 'f-sha' });
    expect(calls).toEqual(['resolve v2', 'resolve v1', 'resolve f', 'mergeBase main f-sha']);
  });

  it('takes the head of a pull request and its merge base from the forge', async () => {
    const calls: string[] = [];
    const forge = { host: 'h', name: 'h', tree: async () => [], text: async () => '', ...history(calls) };
    expect(await resolveRange(forge, REPO, { kind: 'pull', n: 3 })).toEqual({ base: 'pull-base', head: 'pull-head' });
    expect(calls).toEqual(['pull 3']);
  });
});

describe('cachedCommits', () => {
  it('asks the forge once for the history of a commit sha, which never changes', async () => {
    const calls: string[] = [];
    const forge = { host: 'h', name: 'h', tree: async () => [], text: async () => '', ...history(calls) };
    const cache = memoryCache();
    const first = await cachedCommits(forge, REPO, HEAD, 2, cache);
    expect(first.map((c) => c.sha)).toEqual([HEAD, BASE]);
    expect(await cachedCommits(forge, REPO, HEAD, 2, cache)).toEqual(first);
    expect(calls).toEqual([`commits ${HEAD} 2`]);
  });
});

describe('listedCommits', () => {
  /** A forge with a linear history of `authors.length` commits `c0`, `c1`, …, newest first. */
  function linear(authors: string[], calls: string[]): Forge {
    const all = authors.map((author, i) => ({ sha: `c${i}`, parents: i + 1 < authors.length ? [`c${i + 1}`] : [], title: 't', author, date: '' }));
    return {
      ...history([]),
      host: 'h',
      name: 'h',
      tree: async () => [],
      text: async () => '',
      async commits(_repo, sha, n, page = 1) {
        calls.push(`commits ${sha} ${n} ${page}`);
        return all.slice((page - 1) * n, page * n);
      },
    };
  }

  it('asks for further pages until the wanted number of commits stays after the bot commits go', async () => {
    const calls: string[] = [];
    // Pages of 3: [Ann, bot, bot], [bot, Bo, bot], [Cy, Di, Ed], [Fy].
    const forge = linear(['Ann', 'renovate[bot]', 'renovate[bot]', 'dependabot[bot]', 'Bo', 'renovate[bot]', 'Cy', 'Di', 'Ed', 'Fy'], calls);
    const { list } = await listedCommits(forge, REPO, 'c0', 3, memoryCache());
    expect(commitRows(list, null).map((r) => r.sha)).toEqual(['c0', 'c4', 'c6', 'c7', 'c8']);
    expect(calls).toEqual(['commits c0 3 1', 'commits c0 3 2', 'commits c0 3 3']);
  });

  it('tells the page to go on from, and goes on from it; none after the page that ends the history', async () => {
    const calls: string[] = [];
    const forge = linear(['Ann', 'Bo', 'Cy', 'Di', 'Ed'], calls);
    const cache = memoryCache();
    const first = await listedCommits(forge, REPO, 'c0', 2, cache);
    expect(first).toMatchObject({ list: [{ sha: 'c0' }, { sha: 'c1' }], next: 2 });
    const second = await listedCommits(forge, REPO, 'c0', 2, cache, 2);
    expect(second).toMatchObject({ list: [{ sha: 'c2' }, { sha: 'c3' }], next: 3 });
    const last = await listedCommits(forge, REPO, 'c0', 2, cache, 3);
    expect(last).toEqual({ list: [expect.objectContaining({ sha: 'c4' })], next: null });
    expect(calls).toEqual(['commits c0 2 1', 'commits c0 2 2', 'commits c0 2 3']);
  });

  it('stops after 5 pages: a history of bot commits only must not use up the calls that the forge permits', async () => {
    const calls: string[] = [];
    const { list, next } = await listedCommits(linear(Array(40).fill('renovate[bot]'), calls), REPO, 'c0', 2, memoryCache());
    expect(calls).toHaveLength(5);
    expect(list).toHaveLength(10);
    expect(next).toBe(6);
  });
});

describe('commitRows', () => {
  it('gives short sha, title, author and day, and marks the commit whose overlay is shown', () => {
    const list = [
      { sha: HEAD, parents: [BASE, 'x'], title: 'Merge', author: 'Ann', date: '2026-11-01T10:00:00Z' },
      { sha: BASE, parents: [], title: 'First', author: 'Bo', date: '2026-10-01T23:59:00Z' },
    ];
    expect(commitRows(list, { base: BASE, head: HEAD })).toEqual([
      { sha: HEAD, short: 'hhhhhhh', title: 'Merge', author: 'Ann', day: '2026-11-01', current: true },
      { sha: BASE, short: 'bbbbbbb', title: 'First', author: 'Bo', day: '2026-10-01', current: false },
    ]);
    // A range that is not the commit against its first parent marks nothing.
    expect(commitRows(list, { base: 'x', head: HEAD }).map((r) => r.current)).toEqual([false, false]);
    expect(commitRows(list, { base: null, head: BASE }).map((r) => r.current)).toEqual([false, true]);
  });

  it('leaves out the commits of a bot that updates dependencies', () => {
    const list = [
      { sha: 'r1', parents: [HEAD], title: 'Update dependency x', author: 'renovate[bot]', date: '' },
      { sha: HEAD, parents: [BASE], title: 'Work', author: 'Ann', date: '' },
      { sha: 'd1', parents: [], title: 'Bump y', author: 'dependabot[bot]', date: '' },
    ];
    expect(commitRows(list, null).map((r) => r.sha)).toEqual([HEAD]);
  });

  it('leaves out a merge commit that merges a commit of such a bot', () => {
    const list = [
      { sha: 'm2', parents: ['m1', 'w1'], title: 'Merge the work', author: 'Ann', date: '' },
      { sha: 'w1', parents: [BASE], title: 'Work', author: 'Bo', date: '' },
      { sha: 'm1', parents: [BASE, 'r1'], title: 'Merge the update', author: 'Ann', date: '' },
      { sha: 'r1', parents: [BASE], title: 'Update dependency x', author: 'renovate[bot]', date: '' },
      { sha: BASE, parents: [], title: 'First', author: 'Ann', date: '' },
    ];
    expect(commitRows(list, null).map((r) => r.sha)).toEqual(['m2', 'w1', BASE]);
  });

  it('leaves out a merge commit whose title names a branch of such a bot, if the list does not hold the merged commit', () => {
    const merge = (sha: string, title: string) => ({ sha, parents: [BASE, 'not-listed'], title, author: 'Ann', date: '' });
    const list = [
      merge('g', 'Merge pull request #12 from o/renovate/serde-1.x'),
      merge('f', "Merge pull request 'Update x' (#12) from renovate/x into main"),
      merge('d', 'Merge pull request #7 from o/dependabot/npm_and_yarn/vite-7.1.0'),
      merge('w', 'Merge pull request #8 from o/renovate-config-docs'),
      // No merge commit: the title alone does not hide it.
      { sha: 's', parents: [BASE], title: 'Merge the notes on renovate/x', author: 'Ann', date: '' },
    ];
    expect(commitRows(list, null).map((r) => r.sha)).toEqual(['w', 's']);
  });
});

describe('byUpdateBot', () => {
  const by = (author: string) => byUpdateBot({ sha: HEAD, parents: [], title: 't', author, date: '' });

  it('is true for a commit of Renovate as GitHub names it', () => {
    expect(by('renovate[bot]')).toBe(true);
    expect(by('Ann')).toBe(false);
  });

  it('is true for Dependabot, and for the names that a self-hosted bot has by default', () => {
    for (const a of ['dependabot[bot]', 'dependabot-preview[bot]', 'Renovate Bot', 'renovate-bot', 'Mend Renovate', 'dependabot'])
      expect(by(a), a).toBe(true);
    // A person whose name only contains the word stays in the list.
    for (const a of ['Renovate Fan', 'Ann Renovate-Smith', '']) expect(by(a), a).toBe(false);
  });
});

describe('overlayNames', () => {
  it('names both ends by short sha, the missing base of a root commit by none', () => {
    expect(overlayNames({ base: BASE, head: HEAD })).toEqual({ base: 'bbbbbbb', head: 'hhhhhhh' });
    expect(overlayNames({ base: null, head: HEAD })).toEqual({ base: 'none', head: 'hhhhhhh' });
  });
});
