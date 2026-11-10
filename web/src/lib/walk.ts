// Walking a chosen directory without listing the parts that cannot hold inputs (ADR 0015).
import { chooseInputs, type Chosen } from './project';

/** Something that can be read as text; a `File` is one. */
export interface Readable {
  text(): Promise<string>;
}

/** A directory as the walk sees it. The adapters below make one from a browser handle or entry. */
export interface WalkDir {
  name: string;
  entries(): AsyncIterable<WalkEntry>;
}

export type WalkEntry = ({ kind: 'file'; name: string } & Readable) | ({ kind: 'directory' } & WalkDir);

/** The files below `dir` that `isInput` wants, with their paths below `dir`. */
export async function walkInputs(
  dir: WalkDir,
  isInput: (path: string) => boolean,
  onDir: (listed: number) => void = () => {},
): Promise<Chosen<Readable>> {
  const inputs: { path: string; file: Readable }[] = [];
  let listed = 0;
  async function visit(d: WalkDir, prefix: string) {
    onDir(++listed);
    const below: Promise<void>[] = [];
    for await (const e of d.entries()) {
      const path = prefix + e.name;
      if (e.kind === 'directory') {
        // A directory can hold inputs iff a source file in it would be one: the rule is asked, not repeated.
        if (isInput(path + '/X.java')) below.push(visit(e, path + '/'));
      } else if (isInput(path)) inputs.push({ path, file: e });
    }
    // Sibling directories are listed at the same time: each listing waits on the browser.
    await Promise.all(below);
  }
  await visit(dir, '');
  inputs.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return { root: dir.name, inputs };
}

/** The part of `FileSystemDirectoryHandle` (from `showDirectoryPicker`) that the walk uses. */
export interface DirHandle {
  kind: 'directory';
  name: string;
  values(): AsyncIterable<DirHandle | { kind: 'file'; name: string; getFile(): Promise<Readable> }>;
}

export function fromHandle(h: DirHandle): WalkDir {
  return {
    name: h.name,
    async *entries() {
      for await (const c of h.values()) {
        if (c.kind === 'directory') yield { kind: 'directory', ...fromHandle(c) };
        else yield { kind: 'file', name: c.name, text: async () => (await c.getFile()).text() };
      }
    },
  };
}

/** The parts of `FileSystemFileEntry` and `FileSystemDirectoryEntry` (from a drop) that the walk uses. */
export interface FileEntry {
  name: string;
  file(ok: (f: Readable) => void, err?: (e: unknown) => void): void;
}
export interface DirEntry {
  name: string;
  createReader(): { readEntries(ok: (batch: (DirEntry | FileEntry)[]) => void, err?: (e: unknown) => void): void };
}

export function fromEntry(d: DirEntry): WalkDir {
  return {
    name: d.name,
    async *entries() {
      const reader = d.createReader();
      // `readEntries` gives a batch per call (100 in Chromium) and an empty one at the end.
      for (;;) {
        const batch = await new Promise<(DirEntry | FileEntry)[]>((ok, err) => reader.readEntries(ok, err));
        if (batch.length === 0) return;
        for (const e of batch) {
          if ('createReader' in e) yield { kind: 'directory', ...fromEntry(e) };
          else yield { kind: 'file', name: e.name, text: async () => (await new Promise<Readable>((ok, err) => e.file(ok, err))).text() };
        }
      }
    },
  };
}

/** True for the error of a picker that the user closed without a choice. */
export function isCancel(e: unknown): boolean {
  return e instanceof DOMException && e.name === 'AbortError';
}

/** One way into a directory: it gives the inputs once the rule of the indexer is loaded. */
export type Collect = (isInput: (path: string) => boolean, onDir: (listed: number) => void) => Promise<Chosen<Readable>>;

/** The files of an `<input webkitdirectory>`: the browser has listed them all already. */
export const collectFiles =
  (files: File[]): Collect =>
  async (isInput) =>
    chooseInputs(files, (f) => f.webkitRelativePath || f.name, isInput);

/** A directory from the picker or from a drop: only the parts that can hold inputs are listed. */
export const collectDir =
  (dir: WalkDir): Collect =>
  (isInput, onDir) =>
    walkInputs(dir, isInput, onDir);
