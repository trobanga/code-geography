import { describe, expect, it } from 'vitest';
import { cachedTree, fetchRepo, inputsOf, memoryCache, openCache, pool, texts, type Entry, type Forge } from './forge';

const SHA = '0123456789abcdef0123456789abcdef01234567';
const REPO = { owner: 'a', name: 'b' };

/** A forge of files in memory; `calls` records each method call. */
function fakeForge(files: Record<string, string>, host = 'example.org') {
  const calls: string[] = [];
  const forge: Forge = {
    host,
    name: host,
    async resolve(repo) {
      calls.push(`resolve ${repo.ref ?? 'HEAD'}`);
      return { sha: SHA };
    },
    async tree() {
      calls.push('tree');
      return Object.keys(files).map((path): Entry => ({ path, blobSha: `blob-of-${path}` }));
    },
    async text(_repo, sha, path) {
      calls.push(`text ${sha.slice(0, 4)} ${path}`);
      return files[path];
    },
    commits: async () => [],
    mergeBase: async () => SHA,
    pull: async () => ({ base: SHA, head: SHA }),
    branches: async () => [],
    defaultBranch: async () => 'main',
  };
  return { forge, calls };
}

describe('pool', () => {
  it('runs at most `limit` jobs at a time and keeps the order of the results', async () => {
    let running = 0;
    let most = 0;
    const out = await pool([5, 1, 4, 2, 3, 0, 6], 3, async (x) => {
      most = Math.max(most, ++running);
      await new Promise((r) => setTimeout(r, x));
      running--;
      return x * 10;
    });
    expect(out).toEqual([50, 10, 40, 20, 30, 0, 60]);
    expect(most).toBe(3);
  });

  it('starts no new job after one has failed', async () => {
    const started: number[] = [];
    const run = pool([0, 1, 2, 3, 4, 5], 2, async (x) => {
      started.push(x);
      if (x === 1) throw new Error('boom');
      await new Promise((r) => setTimeout(r, 1));
      return x;
    });
    await expect(run).rejects.toThrow('boom');
    await new Promise((r) => setTimeout(r, 20));
    expect(started).toEqual([0, 1]);
  });
});

describe('texts', () => {
  it('asks the forge for each file at the commit and reports progress', async () => {
    const { forge, calls } = fakeForge({ 'src/A.java': 'class A {}', 'B.java': 'class B {}' });
    const progress: string[] = [];
    const entries = [{ path: 'src/A.java', blobSha: 'b1' }, { path: 'B.java', blobSha: 'b2' }];
    const out = await texts(forge, REPO, SHA, entries, memoryCache(), (done, all) => progress.push(`${done}/${all}`));
    expect(out).toEqual([{ path: 'src/A.java', text: 'class A {}' }, { path: 'B.java', text: 'class B {}' }]);
    expect(calls).toEqual(['text 0123 src/A.java', 'text 0123 B.java']);
    expect(progress).toEqual(['1/2', '2/2']);
  });

  it('takes a text from the cache by blob sha, and stores what it fetched', async () => {
    const cache = memoryCache();
    await cache.put('b1', 'class A {}');
    const { forge, calls } = fakeForge({ 'B.java': 'class B {}' });
    const entries = [{ path: 'renamed/A.java', blobSha: 'b1' }, { path: 'B.java', blobSha: 'b2' }];
    const out = await texts(forge, REPO, SHA, entries, cache);
    expect(out.map((o) => o.text)).toEqual(['class A {}', 'class B {}']);
    expect(calls).toEqual(['text 0123 B.java']);
    expect(await cache.get('b2')).toBe('class B {}');
  });
});

describe('cachedTree', () => {
  it('asks the forge once per commit and keeps the trees of two hosts apart', async () => {
    const cache = memoryCache();
    const one = fakeForge({ 'A.java': '' }, 'github.com');
    const other = fakeForge({ 'B.java': '' }, 'forgejo.example.org');
    const first = await cachedTree(one.forge, REPO, SHA, cache);
    expect(await cachedTree(one.forge, REPO, SHA, cache)).toEqual(first);
    expect(one.calls).toEqual(['tree']);
    expect((await cachedTree(other.forge, REPO, SHA, cache)).map((e) => e.path)).toEqual(['B.java']);
  });
});

describe('openCache', () => {
  it('falls back to memory when IndexedDB is missing or refuses to open', async () => {
    const refusing = {
      open: () => {
        throw new DOMException('denied', 'SecurityError');
      },
    } as unknown as IDBFactory;
    const failing = {
      open: () => {
        const req = {} as IDBOpenDBRequest;
        setTimeout(() => (req.onerror as () => void)?.());
        return req;
      },
    } as unknown as IDBFactory;
    for (const factory of [undefined, refusing, failing]) {
      const cache = await openCache(factory);
      await cache.put('k', 'v');
      expect(await cache.get('k')).toBe('v');
    }
  });

  it('clears every entry, as "forget token" asks', async () => {
    const cache = await openCache(undefined);
    await cache.put('blob', 'text');
    await cache.put('tree:h/o/r@s', '[]');
    await cache.clear();
    expect(await cache.get('blob')).toBeUndefined();
    expect(await cache.get('tree:h/o/r@s')).toBeUndefined();
  });
});

describe('inputsOf', () => {
  it('keeps the entries that the rule of the indexer wants, sorted by path', () => {
    const isInput = (p: string) => p.endsWith('.java') || p.endsWith('pom.xml');
    const entries = [
      { path: 'z/Z.java', blobSha: '1' },
      { path: 'README.md', blobSha: '2' },
      { path: 'pom.xml', blobSha: '3' },
      { path: 'a/A.java', blobSha: '4' },
    ];
    expect(inputsOf(entries, isInput).map((e) => e.path)).toEqual(['a/A.java', 'pom.xml', 'z/Z.java']);
  });
});

describe('fetchRepo', () => {
  const isInput = (p: string) => p.endsWith('.java') || p === 'pom.xml';

  it('fails before any file fetch when the repository has no sources of a supported language', async () => {
    const { forge, calls } = fakeForge({ 'pom.xml': '', 'x.py': '' });
    await expect(fetchRepo(forge, REPO, memoryCache(), isInput)).rejects.toThrow('no Java, Rust, Clojure, Elixir or TypeScript sources found in a/b');
    expect(calls.some((c) => c.startsWith('text'))).toBe(false);
  });
});
