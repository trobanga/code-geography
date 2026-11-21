// The git history of a chosen directory (ADR 0039): the files of its `.git` that the reader in
// WASM needs, and the overlay of the uncommitted changes against `HEAD`.
import { pool } from './pool';
import type { FileLike, WalkDir, WalkEntry } from './walk';

/**
 * The most bytes of packs and loose objects that the viewer reads: they stay whole in the
 * memory of the index worker, once as the bytes read and once in WASM (ADR 0039).
 */
export const MAX_OBJECT_BYTES = 512 * 2 ** 20;

/** How many loose objects are read at the same time. */
const LOOSE_READERS = 64;

/** The files of `.git` that the reader takes: `HEAD`, the refs below `refs`, `packed-refs`, the packs and the loose objects. */
export interface GitFiles {
  head: string;
  /** `name`: the path below `.git` (`refs/heads/main`). */
  refs: { name: string; text: string }[];
  packedRefs: string | null;
  /** A `.idx` file and the `.pack` file of the same name. */
  packs: { index: Uint8Array; data: Uint8Array }[];
  /** `id`: the name of the directory and of the file below `objects`. */
  loose: { id: string; bytes: Uint8Array }[];
}

/** What `.git` of a chosen directory gives: nothing, a note for the user instead of an overlay, or a repository. */
export type GitProbe = { kind: 'none' } | { kind: 'note'; note: string } | { kind: 'repo'; files: GitFiles };

const note = (text: string): Extract<GitProbe, { kind: 'note' }> => ({ kind: 'note', note: `no overlay of uncommitted changes: ${text}` });

type FileEntry = Extract<WalkEntry, { kind: 'file' }>;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function list(dir: WalkDir): Promise<WalkEntry[]> {
  const out: WalkEntry[] = [];
  for await (const e of dir.entries()) out.push(e);
  return out;
}

const named = (entries: WalkEntry[], name: string) => entries.find((e) => e.name === name);
const fileIn = (entries: WalkEntry[], name: string) => {
  const e = named(entries, name);
  return e?.kind === 'file' ? e : undefined;
};
const dirIn = (entries: WalkEntry[], name: string) => {
  const e = named(entries, name);
  return e?.kind === 'directory' ? e : undefined;
};

/** The files below `dir`, with their paths from `prefix`. */
async function filesBelow(dir: WalkDir, prefix: string): Promise<{ path: string; file: FileEntry }[]> {
  const out: { path: string; file: FileEntry }[] = [];
  for (const e of await list(dir)) {
    if (e.kind === 'directory') out.push(...(await filesBelow(e, `${prefix}${e.name}/`)));
    else out.push({ path: prefix + e.name, file: e });
  }
  return out;
}

const bytesOf = async (f: FileLike) => new Uint8Array(await f.arrayBuffer());

function sizeText(n: number): string {
  return n >= 2 ** 20 ? `${Math.round(n / 2 ** 20)} MB` : `${n} bytes`;
}

/**
 * Reads the `.git` of `dir`. A `.git` file (a work tree or a submodule) points outside the
 * directory, which the browser cannot reach: a note. Packs above `limit` bytes are refused
 * before anything is read; the loose objects are read in one pass, and a running total that
 * passes `limit` with the packs stops it: a note.
 */
export async function readGit(dir: WalkDir, limit = MAX_OBJECT_BYTES): Promise<GitProbe> {
  try {
    return await readGitFiles(dir, limit);
  } catch (e) {
    // A file that git removed after the listing (gc), or one the browser may not read.
    return note(`cannot read .git: ${e instanceof Error ? e.message : String(e)}`);
  }
}

async function readGitFiles(dir: WalkDir, limit: number): Promise<GitProbe> {
  const dot = named(await list(dir), '.git');
  if (!dot) return { kind: 'none' };
  if (dot.kind === 'file') {
    const text = (await dot.text()).trim();
    const target = text.startsWith('gitdir:') ? text.slice('gitdir:'.length).trim() : text;
    return note(
      `.git is a file that points to ${target}: this is a work tree or a submodule, whose history lies outside the chosen directory. Open the main repository to see the uncommitted changes.`,
    );
  }
  const top = await list(dot);
  const headFile = fileIn(top, 'HEAD');
  if (!headFile) return note('.git has no HEAD');
  const refsDir = dirIn(top, 'refs');
  const packedFile = fileIn(top, 'packed-refs');
  const objectsDir = dirIn(top, 'objects');
  const objects = objectsDir ? await list(objectsDir) : [];

  const packDir = dirIn(objects, 'pack');
  const packEntries = packDir ? await list(packDir) : [];
  const packs = packEntries
    .filter((e) => e.kind === 'file' && e.name.endsWith('.idx'))
    .flatMap((idx) => {
      const data = fileIn(packEntries, idx.name.replace(/\.idx$/, '.pack'));
      return data ? [{ index: idx as FileEntry, data }] : [];
    });
  const loose = (
    await Promise.all(
      objects
        .filter((e): e is Extract<WalkEntry, { kind: 'directory' }> => e.kind === 'directory' && /^[0-9a-f]{2}$/.test(e.name))
        // A loose object has 38 hex digits as its name; git leaves others (`tmp_obj_…`) behind a crash.
        .map(async (d) => (await list(d)).flatMap((f) => (f.kind === 'file' && /^[0-9a-f]{38}$/.test(f.name) ? [{ id: d.name + f.name, file: f }] : []))),
    )
  ).flat();

  const tooMuch = (total: number) =>
    note(`the packs and loose objects of .git take ${sizeText(total)}, over the limit of ${sizeText(limit)} that the viewer reads into memory`);
  // The packs are sized first: they are the bulk, and a pack that is too large is not read at all.
  const packOpened = await Promise.all(packs.flatMap((p) => [p.index, p.data]).map((f) => f.open()));
  let total = packOpened.reduce((sum, f) => sum + f.size, 0);
  if (total > limit) return tooMuch(total);
  const packsRead = Promise.all(packOpened.map(bytesOf));
  packsRead.catch(() => {}); // awaited below, unless the loose objects pass the limit first
  // The loose objects in one pass (open, bytes), a few at a time, beside the packs, with a running
  // total: once it passes the limit nothing more is opened.
  const over = new Error('over the limit');
  const looseRead = await pool(loose, LOOSE_READERS, async (l) => {
    // The length of the bytes, not `File.size`: for the files of an `<input webkitdirectory>`
    // each `size` asks the disk again, which took seconds for 8 700 loose objects (CG-16).
    const b = await bytesOf(await l.file.open());
    total += b.byteLength;
    if (total > limit) throw over;
    return b;
  }).catch((e: unknown) => {
    if (e === over) return null;
    throw e;
  });
  if (!looseRead) return tooMuch(total);
  const read = await packsRead;
  const refs = refsDir ? await filesBelow(refsDir, 'refs/') : [];
  return {
    kind: 'repo',
    files: {
      head: await headFile.text(),
      refs: await Promise.all(refs.map(async (r) => ({ name: r.path, text: await r.file.text() }))),
      packedRefs: packedFile ? await packedFile.text() : null,
      packs: packs.map((_, i) => ({ index: read[2 * i], data: read[2 * i + 1] })),
      loose: loose.map((l, i) => ({ id: l.id, bytes: looseRead[i] })),
    },
  };
}

/**
 * The directory of the files of an `<input webkitdirectory>` (paths `root/...`), for `readGit`:
 * only the files below `root/.git`.
 */
export function filesDir(files: File[]): WalkDir {
  type Node = { dirs: Map<string, Node>; files: Map<string, File> };
  const tree: Node = { dirs: new Map(), files: new Map() };
  for (const f of files) {
    const parts = (f.webkitRelativePath || f.name).split(/[\\/]/).slice(1);
    if (parts[0] !== '.git') continue;
    let at = tree;
    for (const d of parts.slice(0, -1)) {
      if (!at.dirs.has(d)) at.dirs.set(d, { dirs: new Map(), files: new Map() });
      at = at.dirs.get(d)!;
    }
    at.files.set(parts[parts.length - 1], f);
  }
  const dirOf = (name: string, n: Node): WalkDir => ({
    name,
    async *entries() {
      for (const [d, below] of n.dirs) yield { kind: 'directory', ...dirOf(d, below) };
      for (const [f, file] of n.files) yield { kind: 'file', name: f, text: () => file.text(), open: async () => file };
    },
  });
  return dirOf('', tree);
}

/** The sources of a chosen directory, as the index worker takes them. */
export interface LocalSources {
  rootName: string;
  paths: string[];
  texts: string[];
}

type Indexed<R> = { graphText: string; report: R };

/** What `openLocal` needs of the index worker: the plain index, then the overlay against `HEAD`. */
export interface LocalWorker<R> {
  index(req: LocalSources): Promise<Indexed<R>>;
  worktree(git: GitFiles): Promise<Indexed<R> & { base: Record<string, string> }>;
  close(): void;
}

export interface OpenSteps<R> {
  /** Shows the plain index. null: a later open replaced this one, nothing was shown. Else a check whether the directory is still shown. */
  show(plain: Indexed<R>): Promise<(() => boolean) | null>;
  /** The overlay against `HEAD`: the worker now belongs to the caller (it serves the history). */
  attach(overlay: Indexed<R> & { base: Map<string, string> }): Promise<void> | void;
  /** git gives no overlay: the note why, null when the directory has no `.git`. */
  settle(note: string | null): void;
}

/**
 * Opens a chosen directory in two steps (CG-16): the plain index is shown as soon as it is
 * there, while the read of `.git` (`probe`) may still run; then the overlay of the working
 * tree against `HEAD` follows, or a note that says why there is none. 'attached': the worker
 * went to `attach`; 'plain': the plain index stays; 'stale': a later open replaced this one.
 * The worker is closed unless attached.
 */
export async function openLocal<R>(
  sources: LocalSources,
  probe: Promise<GitProbe>,
  worker: LocalWorker<R>,
  steps: OpenSteps<R>,
): Promise<'attached' | 'plain' | 'stale'> {
  // A failed read is a note; handled at once, so it is never an unhandled rejection, whatever else happens.
  const read: Promise<GitProbe> = probe.catch((e) => note(`cannot read .git: ${message(e)}`));
  let attached = false;
  /** Why git gives no overlay; settled after the worker closed. */
  let settled: { note: string | null } | null = null;
  try {
    const still = await steps.show(await worker.index(sources));
    if (!still) return 'stale';
    const found = await read;
    if (!still()) return 'stale';
    if (found.kind !== 'repo') {
      settled = { note: found.kind === 'note' ? found.note : null };
      return 'plain';
    }
    const overlay = await worker.worktree(found.files).catch((e: unknown) => note(message(e)));
    if (!still()) return 'stale';
    if ('kind' in overlay) {
      settled = { note: overlay.note };
      return 'plain';
    }
    try {
      await steps.attach({ graphText: overlay.graphText, report: overlay.report, base: new Map(Object.entries(overlay.base)) });
    } catch (e) {
      settled = { note: note(message(e)).note };
      return 'plain';
    }
    attached = true;
    return 'attached';
  } finally {
    if (!attached) worker.close();
    if (settled) steps.settle(settled.note);
  }
}

