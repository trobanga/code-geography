import { describe, expect, it } from 'vitest';
import { filesDir, readGit, indexLocal, type GitFiles, type GitProbe } from './localGit';
import type { WalkDir, WalkEntry } from './walk';

type Tree = { [name: string]: Tree | string | Uint8Array<ArrayBuffer> };

/** A fake directory; `read` collects the paths of the files whose content was read. */
function fakeDir(name: string, tree: Tree, read: string[] = [], at = ''): WalkDir {
  return {
    name,
    async *entries(): AsyncIterable<WalkEntry> {
      for (const [n, v] of Object.entries(tree)) {
        const path = at + n;
        if (typeof v === 'string' || v instanceof Uint8Array) {
          const blob = new Blob([v]);
          const open = async () => ({
            size: blob.size,
            text: () => (read.push(path), blob.text()),
            arrayBuffer: () => (read.push(path), blob.arrayBuffer()),
          });
          yield { kind: 'file', name: n, text: async () => (await open()).text(), open };
        } else yield { kind: 'directory', ...fakeDir(n, v, read, `${path}/`) };
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

  it('refuses objects larger than the limit before it reads them', async () => {
    const read: string[] = [];
    const probe = await readGit(fakeDir('repo', { '.git': GIT }, read), 5);

    expect(probe.kind).toBe('note');
    expect((probe as { note: string }).note).toMatch(/7 bytes.*limit of 5 bytes/);
    expect(read.filter((p) => p.startsWith('.git/objects'))).toEqual([]);
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

describe('indexLocal', () => {
  const sources = { rootName: 'repo', paths: ['A.java'], texts: ['class A {}'] };
  const indexed = { graphText: '{"nodes":[],"edges":[]}', report: { calls: {} as never } };
  const files: GitFiles = { head: '', refs: [], packedRefs: null, packs: [], loose: [] };
  const run = (worktree: () => Promise<typeof indexed & { base: Record<string, string> }>) => ({
    index: async () => indexed,
    worktree,
  });

  it('indexes a directory that is no repository as before', async () => {
    const r = await indexLocal(sources, { kind: 'none' }, run(() => Promise.reject(new Error('not asked'))));
    expect(r).toEqual({ ...indexed, note: null, base: null });
  });

  it('builds the overlay against HEAD for a repository, with the texts at HEAD', async () => {
    const overlay = { graphText: '{"diff":{}}', report: indexed.report, base: { 'A.java': 'class A' } };
    const r = await indexLocal(sources, { kind: 'repo', files }, run(async () => overlay));
    expect(r).toEqual({ graphText: overlay.graphText, report: overlay.report, note: null, base: new Map([['A.java', 'class A']]) });
  });

  it('indexes with the note of the probe', async () => {
    const probe: GitProbe = { kind: 'note', note: 'a work tree' };
    expect((await indexLocal(sources, probe, run(() => Promise.reject(new Error('not asked'))))).note).toBe('a work tree');
  });

  it('indexes with a note when git cannot give the overlay', async () => {
    const r = await indexLocal(sources, { kind: 'repo', files }, run(() => Promise.reject(new Error('no commit at HEAD: no ref refs/heads/main'))));
    expect(r).toEqual({ ...indexed, note: 'no overlay of uncommitted changes: no commit at HEAD: no ref refs/heads/main', base: null });
  });
});
