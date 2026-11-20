// The git history of a chosen directory (ADR 0039): the files of its `.git` that the reader in
// WASM needs, and the overlay of the uncommitted changes against `HEAD`.
import type { FileLike, WalkDir, WalkEntry } from './walk';

/**
 * The most bytes of packs and loose objects that the viewer reads: they stay whole in the
 * memory of the index worker, once as the bytes read and once in WASM (ADR 0039).
 */
export const MAX_OBJECT_BYTES = 512 * 2 ** 20;

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
 * directory, which the browser cannot reach: a note. Packs and loose objects above `limit`
 * bytes in all are refused before they are read: a note.
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

  // The sizes first: a pack that is too large is not read at all.
  const opened = await Promise.all([...packs.flatMap((p) => [p.index, p.data]), ...loose.map((l) => l.file)].map((f) => f.open()));
  const total = opened.reduce((sum, f) => sum + f.size, 0);
  if (total > limit) {
    return note(
      `the packs and loose objects of .git take ${sizeText(total)}, over the limit of ${sizeText(limit)} that the viewer reads into memory`,
    );
  }
  const read = await Promise.all(opened.map(bytesOf));
  const refs = refsDir ? await filesBelow(refsDir, 'refs/') : [];
  return {
    kind: 'repo',
    files: {
      head: await headFile.text(),
      refs: await Promise.all(refs.map(async (r) => ({ name: r.path, text: await r.file.text() }))),
      packedRefs: packedFile ? await packedFile.text() : null,
      packs: packs.map((_, i) => ({ index: read[2 * i], data: read[2 * i + 1] })),
      loose: loose.map((l, i) => ({ id: l.id, bytes: read[2 * packs.length + i] })),
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

/** The two ways to index: plain, or the overlay against `HEAD` with the texts at `HEAD` (`base`). */
export interface LocalRun<R> {
  index(req: LocalSources): Promise<Indexed<R>>;
  worktree(req: LocalSources & { git: GitFiles }): Promise<Indexed<R> & { base: Record<string, string> }>;
}

/**
 * Indexes a chosen directory: with a repository, as the overlay of the working tree against
 * `HEAD`; without, or when git cannot give `HEAD`, plainly, with a note that says why.
 */
export async function indexLocal<R>(
  sources: LocalSources,
  probe: GitProbe,
  run: LocalRun<R>,
): Promise<Indexed<R> & { note: string | null; base: Map<string, string> | null }> {
  if (probe.kind === 'repo') {
    try {
      const r = await run.worktree({ ...sources, git: probe.files });
      return { graphText: r.graphText, report: r.report, note: null, base: new Map(Object.entries(r.base)) };
    } catch (e) {
      const plain = await run.index(sources);
      return { ...plain, note: note(e instanceof Error ? e.message : String(e)).note, base: null };
    }
  }
  const plain = await run.index(sources);
  return { ...plain, note: probe.kind === 'note' ? probe.note : null, base: null };
}
