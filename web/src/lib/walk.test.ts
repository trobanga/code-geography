import { describe, expect, it } from 'vitest';
import { fromEntry, fromHandle, isCancel, walkInputs, type DirEntry, type DirHandle, type FileEntry, type Readable, type WalkDir, type WalkEntry } from './walk';

// The rule of the indexer (`is_input` in crates/index-java), written here only as a test double.
const isInput = (p: string) => {
  const parts = p.split('/');
  const name = parts.pop()!;
  return (name === 'pom.xml' || (name.length > 5 && name.endsWith('.java'))) && parts.every((d) => !d.startsWith('.') && d !== 'target');
};

type Tree = { [name: string]: Tree | string };

/** A fake directory; `opened` collects the paths of the directories whose entries were listed. */
function fakeDir(name: string, tree: Tree, opened: string[] = [], at = name): WalkDir {
  return {
    name,
    async *entries(): AsyncIterable<WalkEntry> {
      opened.push(at);
      for (const [n, v] of Object.entries(tree)) {
        if (typeof v === 'string') yield { kind: 'file', name: n, text: async () => v };
        else yield { kind: 'directory', ...fakeDir(n, v, opened, `${at}/${n}`) };
      }
    },
  };
}

describe('walkInputs', () => {
  it('gives the files that the indexer wants, with paths below the chosen directory', async () => {
    const dir = fakeDir('repo', { 'pom.xml': '', 'README.md': '#', src: { a: { 'A.java': 'class A {}' } } });
    const r = await walkInputs(dir, isInput);
    expect(r.root).toBe('repo');
    expect(r.inputs.map((i) => i.path)).toEqual(['pom.xml', 'src/a/A.java']);
    expect(await r.inputs[1].file.text()).toBe('class A {}');
  });

  it('does not list a directory that the rule of the indexer excludes', async () => {
    const opened: string[] = [];
    const dir = fakeDir('repo', { target: { 'A.java': '' }, '.git': { x: '' }, src: { 'B.java': '' } }, opened);
    const r = await walkInputs(dir, isInput);
    expect(r.inputs.map((i) => i.path)).toEqual(['src/B.java']);
    expect(opened).toEqual(['repo', 'repo/src']);
  });

  it('does not list node_modules, which the rule of the indexer accepts', async () => {
    const opened: string[] = [];
    const dir = fakeDir('repo', { docs: { node_modules: { pkg: { 'index.js': '' } } }, 'A.java': '' }, opened);
    await walkInputs(dir, isInput);
    expect(opened).toEqual(['repo', 'repo/docs']);
  });

  it('reports the running number of listed directories', async () => {
    const counts: number[] = [];
    const dir = fakeDir('repo', { a: { b: {} }, target: {}, c: {} });
    await walkInputs(dir, isInput, (n) => counts.push(n));
    expect(counts).toEqual([1, 2, 3, 4]);
  });
});

/** A fake `FileSystemDirectoryHandle` tree, as `showDirectoryPicker` gives it. */
function fakeHandle(name: string, tree: Tree): DirHandle {
  return {
    kind: 'directory',
    name,
    async *values() {
      for (const [n, v] of Object.entries(tree)) {
        if (typeof v === 'string') yield { kind: 'file' as const, name: n, getFile: async () => ({ text: async () => v }) };
        else yield fakeHandle(n, v);
      }
    },
  };
}

describe('fromHandle', () => {
  it('walks a directory handle', async () => {
    const h = fakeHandle('repo', { node_modules: { 'X.java': '' }, src: { 'A.java': 'class A {}' }, 'pom.xml': '' });
    const r = await walkInputs(fromHandle(h), isInput);
    expect(r.root).toBe('repo');
    expect(r.inputs.map((i) => i.path)).toEqual(['pom.xml', 'src/A.java']);
    expect(await r.inputs[1].file.text()).toBe('class A {}');
  });
});

/** A fake `FileSystemDirectoryEntry` tree, as a drop gives it; `readEntries` returns at most two entries per call. */
function fakeEntry(name: string, tree: Tree): DirEntry {
  return {
    name,
    createReader() {
      const all: (DirEntry | FileEntry)[] = Object.entries(tree).map(([n, v]) =>
        typeof v === 'string'
          ? { name: n, file: (ok: (f: Readable) => void) => setTimeout(() => ok({ text: async () => v })) }
          : fakeEntry(n, v),
      );
      return { readEntries: (ok: (es: (DirEntry | FileEntry)[]) => void) => setTimeout(() => ok(all.splice(0, 2))) };
    },
  };
}

describe('fromEntry', () => {
  it('walks a dropped directory entry and reads it until the reader gives an empty batch', async () => {
    const e = fakeEntry('repo', { 'A.java': 'a', 'B.java': 'b', 'C.java': 'c', target: { 'D.java': '' }, src: { 'E.java': 'e' } });
    const r = await walkInputs(fromEntry(e), isInput);
    expect(r.root).toBe('repo');
    expect(r.inputs.map((i) => i.path)).toEqual(['A.java', 'B.java', 'C.java', 'src/E.java']);
    expect(await r.inputs[3].file.text()).toBe('e');
  });
});

describe('isCancel', () => {
  it('takes an AbortError for a cancelled picker and any other error for an error', () => {
    expect(isCancel(new DOMException('The user aborted a request.', 'AbortError'))).toBe(true);
    expect(isCancel(new DOMException('denied', 'NotAllowedError'))).toBe(false);
    expect(isCancel(new Error('AbortError'))).toBe(false);
  });
});
