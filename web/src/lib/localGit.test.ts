import { describe, expect, it } from 'vitest';
import { filesDir, readGit, openLocal, type GitFiles, type GitProbe } from './localGit';
import type { WalkDir, WalkEntry } from './walk';

type Tree = { [name: string]: Tree | string | Uint8Array<ArrayBuffer> };

/** A fake directory; `read` collects the paths of the files whose content was read. */
function fakeDir(name: string, tree: Tree, read: string[] = [], at = '', opened: string[] = []): WalkDir {
  return {
    name,
    async *entries(): AsyncIterable<WalkEntry> {
      for (const [n, v] of Object.entries(tree)) {
        const path = at + n;
        if (typeof v === 'string' || v instanceof Uint8Array) {
          const blob = new Blob([v]);
          const open = async () => (opened.push(path), {
            size: blob.size,
            text: () => (read.push(path), blob.text()),
            arrayBuffer: () => (read.push(path), blob.arrayBuffer()),
          });
          yield { kind: 'file', name: n, text: async () => (await open()).text(), open };
        } else yield { kind: 'directory', ...fakeDir(n, v, read, `${path}/`, opened) };
      }
    },
  };
}

const bytes = (...b: number[]) => new Uint8Array(b);

const GIT: Tree = {
  HEAD: 'ref: refs/heads/main\n',
  'packed-refs': '# pack-refs with: peeled\nabc refs/tags/v1\n',
  config: '[core]\n',
  refs: { heads: { main: '1111\n', 'feature': { x: '2222\n' } }, tags: {} },
  objects: {
    pack: { 'pack-1.idx': bytes(1), 'pack-1.pack': bytes(2, 2), 'pack-1.rev': bytes(9), 'pack-2.keep': '' },
    info: { packs: 'P pack-1.pack\n' },
    ab: { ['cdef'.padEnd(38, '0')]: bytes(3) },
    '0f': { ['00'.padEnd(38, '0')]: bytes(4, 4, 4) },
  },
};

describe('readGit', () => {
  it('finds no repository in a directory without .git', async () => {
    expect(await readGit(fakeDir('repo', { src: { 'A.java': '' } }))).toEqual({ kind: 'none' });
  });

  it('reads HEAD, the refs, the packed refs, the packs and the loose objects of .git', async () => {
    const read: string[] = [];
    const probe = await readGit(fakeDir('repo', { '.git': GIT, src: { 'A.java': 'class A {}' } }, read));

    expect(probe.kind).toBe('repo');
    const files = (probe as { files: GitFiles }).files;
    expect(files.head).toBe('ref: refs/heads/main\n');
    expect(files.refs.sort((a, b) => a.name.localeCompare(b.name))).toEqual([
      { name: 'refs/heads/feature/x', text: '2222\n' },
      { name: 'refs/heads/main', text: '1111\n' },
    ]);
    expect(files.packedRefs).toBe('# pack-refs with: peeled\nabc refs/tags/v1\n');
    expect(files.packs).toEqual([{ index: bytes(1), data: bytes(2, 2) }]);
    expect(files.loose.sort((a, b) => a.id.localeCompare(b.id))).toEqual([
      { id: '0f' + '00'.padEnd(38, '0'), bytes: bytes(4, 4, 4) },
      { id: 'ab' + 'cdef'.padEnd(38, '0'), bytes: bytes(3) },
    ]);
    expect(read.filter((p) => !p.startsWith('.git/'))).toEqual([]);
    expect(read).not.toContain('.git/config');
  });

  it('gives a note, not a repository, when .git is a file: a work tree or a submodule', async () => {
    const probe = await readGit(fakeDir('wt', { '.git': 'gitdir: /home/u/repo/.git/worktrees/wt\n' }));

    expect(probe.kind).toBe('note');
    expect((probe as { note: string }).note).toContain('/home/u/repo/.git/worktrees/wt');
    expect((probe as { note: string }).note).toContain('work tree or a submodule');
  });

  it('opens and reads each loose object once', async () => {
    const read: string[] = [];
    const opened: string[] = [];
    await readGit(fakeDir('repo', { '.git': GIT }, read, '', opened));

    const loose = ['.git/objects/ab/' + 'cdef'.padEnd(38, '0'), '.git/objects/0f/' + '00'.padEnd(38, '0')];
    for (const path of loose) {
      expect(opened.filter((p) => p === path)).toHaveLength(1);
      expect(read.filter((p) => p === path)).toHaveLength(1);
    }
  });

  it('refuses packs larger than the limit before it opens or reads any loose object', async () => {
    const read: string[] = [];
    const opened: string[] = [];
    const probe = await readGit(fakeDir('repo', { '.git': GIT }, read, '', opened), 2);

    expect(probe.kind).toBe('note');
    expect((probe as { note: string }).note).toMatch(/3 bytes.*limit of 2 bytes/);
    expect(read.filter((p) => p.startsWith('.git/objects'))).toEqual([]);
    expect(opened.filter((p) => /^\.git\/objects\/[0-9a-f]{2}\//.test(p))).toEqual([]);
  });

  it('stops reading loose objects once they pass the limit together with the packs', async () => {
    const many: Tree = {};
    for (let i = 0; i < 300; i++) many[i.toString(16).padStart(38, '0')] = bytes(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
    const git: Tree = { HEAD: 'ref: refs/heads/main\n', objects: { ab: many } };
    const read: string[] = [];
    const opened: string[] = [];

    const probe = await readGit(fakeDir('repo', { '.git': git }, read, '', opened), 100);

    expect(probe.kind).toBe('note');
    expect((probe as { note: string }).note).toMatch(/bytes.*limit of 100 bytes/);
    expect(opened.length).toBeLessThan(300);
  });

  it('counts the packs and the loose objects against one limit', async () => {
    const git: Tree = {
      HEAD: 'ref: refs/heads/main\n',
      objects: { pack: { 'p.idx': new Uint8Array(30), 'p.pack': new Uint8Array(30) }, ab: { ['1'.padStart(38, '0')]: new Uint8Array(30), ['2'.padStart(38, '0')]: new Uint8Array(30) } },
    };
    // Packs 60 of 100 bytes: within the limit alone; with the loose objects 120.
    const over = await readGit(fakeDir('repo', { '.git': git }), 100);
    expect(over.kind).toBe('note');
    expect((over as { note: string }).note).toMatch(/limit of 100 bytes/);
    // Exactly at the limit is within it.
    expect((await readGit(fakeDir('repo', { '.git': git }), 120)).kind).toBe('repo');
  });

  it('skips a file below objects that is no loose object, such as the temporary file of a crashed commit', async () => {
    const id = 'cdef'.padEnd(38, '0');
    const git = { HEAD: 'ref: refs/heads/main\n', objects: { ab: { [id]: bytes(3), tmp_obj_AbC123: bytes(5) } } };
    const probe = await readGit(fakeDir('repo', { '.git': git }));
    expect((probe as { files: GitFiles }).files.loose).toEqual([{ id: `ab${id}`, bytes: bytes(3) }]);
  });

  it('gives a note, not a failure, when a file of .git cannot be read', async () => {
    const dir: WalkDir = {
      name: 'repo',
      async *entries() {
        yield {
          kind: 'directory',
          name: '.git',
          async *entries() {
            yield { kind: 'file', name: 'HEAD', text: () => Promise.reject(new Error('A requested file could not be found')), open: () => Promise.reject(new Error('gone')) };
          },
        };
      },
    };
    expect(await readGit(dir)).toEqual({ kind: 'note', note: 'no overlay of uncommitted changes: cannot read .git: A requested file could not be found' });
  });

  it('gives a note for a .git without HEAD', async () => {
    const probe = await readGit(fakeDir('repo', { '.git': { objects: {} } }));
    expect(probe).toEqual({ kind: 'note', note: 'no overlay of uncommitted changes: .git has no HEAD' });
  });
});

describe('filesDir', () => {
  it('makes the directory of .git from the files of an <input webkitdirectory>', async () => {
    const file = (path: string, text: string) => Object.assign(new File([text], path.split('/').pop()!), { webkitRelativePath: path });
    const files = [
      file('repo/.git/HEAD', 'ref: refs/heads/main\n'),
      file('repo/.git/refs/heads/main', '1111\n'),
      file(`repo/.git/objects/ab/${'cdef'.padEnd(38, '0')}`, 'x'),
      file('repo/src/A.java', 'class A {}'),
    ];

    const probe = await readGit(filesDir(files));

    expect(probe.kind).toBe('repo');
    const git = (probe as { files: GitFiles }).files;
    expect(git.refs).toEqual([{ name: 'refs/heads/main', text: '1111\n' }]);
    expect(git.loose).toEqual([{ id: 'ab' + 'cdef'.padEnd(38, '0'), bytes: new TextEncoder().encode('x') }]);
  });
});

describe('openLocal', () => {
  const sources = { rootName: 'repo', paths: ['A.java'], texts: ['class A {}'] };
  const indexed = { graphText: '{"nodes":[],"edges":[]}', report: { calls: {} as never } };

  /** A worker that the test answers; `events` records what the steps and the worker did, in order. */
  function setup(opts: { overlay?: () => Promise<typeof indexed & { base: Record<string, string> }>; show?: () => (() => boolean) | null } = {}) {
    const events: string[] = [];
    let shown: typeof indexed | null = null;
    const worker = {
      index: async () => (events.push('index'), indexed),
      worktree: async (files: GitFiles) => (events.push('worktree'), (opts.overlay ?? (() => Promise.reject(new Error('not asked'))))()),
      close: () => void events.push('close'),
    };
    const steps = {
      show: async (plain: typeof indexed) => {
        events.push('show');
        shown = plain;
        return opts.show ? opts.show() : () => true;
      },
      attach: (o: unknown) => void events.push('attach'),
      settle: (note: string | null) => void events.push(`settle ${note}`),
    };
    return { events, worker, steps, shown: () => shown };
  }
  const deferred = <T>() => {
    let resolve!: (v: T) => void;
    const promise = new Promise<T>((r) => (resolve = r));
    return { promise, resolve };
  };
  const flush = () => new Promise((r) => setTimeout(r, 0));

  it('shows the plain index while git is still being read', async () => {
    const t = setup();
    const git = deferred<GitProbe>();

    const done = openLocal(sources, git.promise, t.worker, t.steps);
    await flush();

    expect(t.events).toEqual(['index', 'show']);
    expect(t.shown()).toBe(indexed);
    git.resolve({ kind: 'none' });
    await done;
  });

  it('attaches the worker with the overlay and its base as a Map when git gives a repository', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    const overlay = { graphText: '{"diff":{}}', report: indexed.report, base: { 'A.java': 'class A' } };
    const t = setup({ overlay: async () => overlay });
    const attached: unknown[] = [];
    t.steps.attach = (o) => void attached.push(o);

    const result = await openLocal(sources, Promise.resolve({ kind: 'repo', files }), t.worker, t.steps);

    expect(result).toBe('attached');
    expect(attached).toEqual([{ graphText: overlay.graphText, report: overlay.report, base: new Map([['A.java', 'class A']]) }]);
    expect(t.events).toEqual(['index', 'show', 'worktree']);
  });
  it('settles with no note and closes the worker when the directory has no .git', async () => {
    const t = setup();

    const result = await openLocal(sources, Promise.resolve({ kind: 'none' }), t.worker, t.steps);

    expect(result).toBe('plain');
    expect(t.events).toEqual(['index', 'show', 'close', 'settle null']);
  });

  it('settles with the note of the probe and closes the worker', async () => {
    const t = setup();

    const result = await openLocal(sources, Promise.resolve({ kind: 'note', note: 'a work tree' }), t.worker, t.steps);

    expect(result).toBe('plain');
    expect(t.events).toEqual(['index', 'show', 'close', 'settle a work tree']);
  });
  it('settles with a note, as the viewer words it, when git cannot give the overlay', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    const t = setup({ overlay: () => Promise.reject(new Error('no commit at HEAD: no ref refs/heads/main')) });

    const result = await openLocal(sources, Promise.resolve({ kind: 'repo', files }), t.worker, t.steps);

    expect(result).toBe('plain');
    expect(t.events).toEqual(['index', 'show', 'worktree', 'close', 'settle no overlay of uncommitted changes: no commit at HEAD: no ref refs/heads/main']);
  });
  it('takes a rejected read of .git as a note', async () => {
    const t = setup();
    const result = await openLocal(sources, Promise.reject(new Error('NotAllowedError')), t.worker, t.steps);
    expect(result).toBe('plain');
    expect(t.events).toEqual(['index', 'show', 'close', 'settle no overlay of uncommitted changes: cannot read .git: NotAllowedError']);
  });

  it('closes the worker and rejects when the index fails, with no unhandled rejection of the read of .git', async () => {
    const failing = setup();
    failing.worker.index = () => Promise.reject(new Error('index broke'));
    await expect(openLocal(sources, Promise.reject(new Error('late')), failing.worker, failing.steps)).rejects.toThrow('index broke');
    expect(failing.events).toEqual(['close']);
    await flush();
  });

  it('closes the worker and settles with a note when attaching the overlay fails', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    const overlay = { graphText: '{}', report: indexed.report, base: {} };
    const t = setup({ overlay: async () => overlay });
    t.steps.attach = () => {
      throw new Error('attach broke');
    };

    const result = await openLocal(sources, Promise.resolve({ kind: 'repo', files }), t.worker, t.steps);

    expect(result).toBe('plain');
    expect(t.events).toEqual(['index', 'show', 'worktree', 'close', 'settle no overlay of uncommitted changes: attach broke']);
  });

  it('closes the worker and rejects when showing the plain index fails', async () => {
    const t = setup({
      show: () => {
        throw new Error('show broke');
      },
    });
    await expect(openLocal(sources, Promise.resolve({ kind: 'none' }), t.worker, t.steps)).rejects.toThrow('show broke');
    expect(t.events).toEqual(['index', 'show', 'close']);
  });

  it('does no git work and closes the worker when a later open replaced this one before the plain index showed', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    const t = setup({ show: () => null });

    const result = await openLocal(sources, Promise.resolve({ kind: 'repo', files }), t.worker, t.steps);

    expect(result).toBe('stale');
    expect(t.events).toEqual(['index', 'show', 'close']);
  });

  it('closes the worker and settles nothing when the directory is replaced before git is read', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    let shown = true;
    const t = setup({ show: () => () => shown });
    const git = deferred<GitProbe>();

    const done = openLocal(sources, git.promise, t.worker, t.steps);
    await flush();
    shown = false;
    git.resolve({ kind: 'repo', files });

    expect(await done).toBe('stale');
    expect(t.events).toEqual(['index', 'show', 'close']);
  });

  it('closes the worker and attaches nothing when the directory is replaced while the overlay is built', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    let shown = true;
    const overlay = deferred<typeof indexed & { base: Record<string, string> }>();
    const t = setup({ show: () => () => shown, overlay: () => overlay.promise });

    const done = openLocal(sources, Promise.resolve({ kind: 'repo', files }), t.worker, t.steps);
    await flush();
    shown = false;
    overlay.resolve({ ...indexed, base: {} });

    expect(await done).toBe('stale');
    expect(t.events).toEqual(['index', 'show', 'worktree', 'close']);
  });

  it('settles nothing when the directory is replaced and git fails', async () => {
    const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
    let shown = true;
    const t = setup({
      show: () => () => shown,
      overlay: async () => {
        shown = false;
        throw new Error('no commit at HEAD');
      },
    });

    const result = await openLocal(sources, Promise.resolve({ kind: 'repo', files }), t.worker, t.steps);

    expect(result).toBe('stale');
    expect(t.events).toEqual(['index', 'show', 'worktree', 'close']);
  });
});


describe('readGit, less usual .git directories', () => {
  it('names the whole text of a .git file that has no gitdir prefix', async () => {
    const probe = await readGit(fakeDir('wt', { '.git': '/home/u/elsewhere\n' }));

    expect(probe).toMatchObject({ kind: 'note', note: expect.stringContaining('points to /home/u/elsewhere:') });
  });

  it('leaves out a pack index that has no pack beside it', async () => {
    const git: Tree = { HEAD: 'ref: refs/heads/main\n', objects: { pack: { 'pack-1.idx': bytes(1), 'pack-2.idx': bytes(5), 'pack-2.pack': bytes(6) } } };

    const probe = await readGit(fakeDir('repo', { '.git': git }));

    expect((probe as { files: GitFiles }).files.packs).toEqual([{ index: bytes(5), data: bytes(6) }]);
  });

  it('words the size of the packs in megabytes once it passes a megabyte', async () => {
    const git: Tree = { HEAD: 'ref: refs/heads/main\n', objects: { pack: { 'pack-1.idx': bytes(1), 'pack-1.pack': new Uint8Array(new ArrayBuffer(3 * 2 ** 20)) } } };

    const probe = await readGit(fakeDir('repo', { '.git': git }), 2 * 2 ** 20);

    expect(probe).toMatchObject({ kind: 'note', note: expect.stringMatching(/take 3 MB, over the limit of 2 MB/) });
  });

  it('gives a note, also when a loose object and a pack cannot be read, or when the cause is no Error', async () => {
    const id = 'cdef'.padEnd(38, '0');
    const file = (name: string, read: () => Promise<never>) => ({
      kind: 'file' as const,
      name,
      text: async () => '',
      open: async () => ({ size: 1, text: read, arrayBuffer: read }),
    });
    const dir = (name: string, ...entries: WalkEntry[]): WalkEntry => ({ kind: 'directory', name, async *entries() { yield* entries; } });
    const root = (...entries: WalkEntry[]): WalkDir => ({ name: 'repo', async *entries() { yield dir('.git', ...entries); } });
    const head: WalkEntry = { kind: 'file', name: 'HEAD', text: async () => 'ref: refs/heads/main\n', open: async () => ({ size: 1, text: async () => '', arrayBuffer: async () => new ArrayBuffer(0) }) };
    const gone = () => Promise.reject(new Error('gone'));

    const probe = await readGit(root(head, dir('objects', dir('pack', file('pack-1.idx', gone), file('pack-1.pack', gone)), dir('ab', file(id, gone)))));
    expect(probe).toEqual({ kind: 'note', note: 'no overlay of uncommitted changes: cannot read .git: gone' });

    const odd = { ...head, text: () => Promise.reject('plain text') };
    expect(await readGit(root(odd))).toEqual({ kind: 'note', note: 'no overlay of uncommitted changes: cannot read .git: plain text' });
  });
});

describe('openLocal, causes that are no Error', () => {
  const sources = { rootName: 'repo', paths: [], texts: [] };
  const indexed = { graphText: '{}', report: { calls: {} as never } };
  const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
  const steps = (settled: (string | null)[]) => ({ show: async () => () => true, attach: () => {}, settle: (n: string | null) => void settled.push(n) });

  it('takes a read of .git that rejects with a text as a note', async () => {
    const settled: (string | null)[] = [];
    const worker = { index: async () => indexed, worktree: async () => Promise.reject(new Error('unused')), close: () => {} };

    await openLocal(sources, Promise.reject('denied'), worker, steps(settled));

    expect(settled).toEqual(['no overlay of uncommitted changes: cannot read .git: denied']);
  });

  it('takes an overlay that rejects with a text as a note', async () => {
    const settled: (string | null)[] = [];
    const worker = { index: async () => indexed, worktree: () => Promise.reject('no HEAD'), close: () => {} };

    const result = await openLocal(sources, Promise.resolve({ kind: 'repo', files }), worker, steps(settled));

    expect(result).toBe('plain');
    expect(settled).toEqual(['no overlay of uncommitted changes: no HEAD']);
  });
});
