// The commit overlay (ADR 0009, 0012) of a repository on a forge (ADR 0021): the changed files
// from a compare of two trees, the inputs of both sides from the cache or else the forge.
import { cachedTree, inputsOf, texts, type Commit, type Entry, type Forge, type Repo, type TextCache } from './forge';
import { inputError } from './project';
import type { NodeChange } from './types';

/**
 * An overlay asked for in the input field (ADR 0021); its head is `repo.ref`, absent for a
 * pull request. `commit`: against the first parent; `range`: against `base`, or with
 * `mergeBase` against the merge base of `base` and the head.
 */
export type OverlaySpec = { kind: 'commit' } | { kind: 'range'; base: string; mergeBase: boolean } | { kind: 'pull'; n: number };

/** The text of the input field or of the hash read by a forge: a repository, maybe an overlay, or why it is not one. */
export type Parsed = { ok: true; repo: Repo; note?: string; overlay?: OverlaySpec } | { ok: false; error: string };

export interface Changed {
  path: string;
  change: NodeChange;
}

const byPath = (a: { path: string }, b: { path: string }) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);

/**
 * The files that differ between two trees, by blob sha, sorted by path as git lists them. No
 * rename detection (`git diff --no-renames`). A root commit has the empty tree as base.
 */
export function treeChanges(base: Entry[], head: Entry[]): Changed[] {
  const before = new Map(base.map((e) => [e.path, e.blobSha]));
  const out: Changed[] = [];
  for (const h of head) {
    const b = before.get(h.path);
    if (b === undefined) out.push({ path: h.path, change: 'added' });
    else if (b !== h.blobSha) out.push({ path: h.path, change: 'modified' });
    before.delete(h.path);
  }
  for (const path of before.keys()) out.push({ path, change: 'removed' });
  return out.sort(byPath);
}

/** Paths and texts of one side, in the order of the indexer. */
export interface Side {
  paths: string[];
  texts: string[];
}

export interface OverlayInputs {
  /** The repository name: the root name of the graph. */
  root: string;
  base: Side;
  head: Side;
  /** All changed files, inputs of the indexer or not. */
  changed: Changed[];
}

/** Two commits of an overlay; `base` null: the head is a root commit, compared with nothing. */
export interface Range {
  base: string | null;
  head: string;
}

/**
 * The inputs of `diffSources` for two commits: two `tree` calls (none for cached commits), then
 * the files. The head goes first, so the base finds each unchanged file in the cache by blob sha.
 */
export async function fetchOverlay(
  forge: Forge,
  repo: Repo,
  range: Range,
  cache: TextCache,
  isInput: (path: string) => boolean,
  onPhase: (phase: string) => void = () => {},
): Promise<OverlayInputs> {
  const id = `${repo.owner}/${repo.name}`;
  const short = (sha: string) => sha.slice(0, 7);
  onPhase(`listing the files of ${id}@${short(range.head)}`);
  const headTree = await cachedTree(forge, repo, range.head, cache);
  const baseTree = range.base === null ? [] : await cachedTree(forge, repo, range.base, cache);
  const headIn = inputsOf(headTree, isInput);
  const baseIn = inputsOf(baseTree, isInput);
  const err = inputError({ root: id, inputs: [...headIn, ...baseIn].map((e) => ({ path: e.path, file: e })) });
  if (err) throw new Error(err);
  const side = async (sha: string, entries: Entry[], what: string): Promise<Side> => {
    const got = await texts(forge, repo, sha, entries, cache, (done, all) => onPhase(`fetched ${done} / ${all} files of ${what}`));
    return { paths: got.map((g) => g.path), texts: got.map((g) => g.text) };
  };
  const head = await side(range.head, headIn, `${id}@${short(range.head)}`);
  const base = range.base === null ? { paths: [], texts: [] } : await side(range.base, baseIn, `${id}@${short(range.base)}`);
  return { root: repo.name, base, head, changed: treeChanges(baseTree, headTree) };
}

/** The two commits of an overlay asked for in the input field; `repo.ref` is the head. */
export async function resolveRange(forge: Forge, repo: Repo, spec: OverlaySpec): Promise<Range> {
  if (spec.kind === 'commit') {
    // One call gives the sha and the parents, also for a short sha.
    const [c] = await forge.commits(repo, repo.ref ?? 'HEAD', 1);
    return { base: c.parents[0] ?? null, head: c.sha };
  }
  if (spec.kind === 'range') {
    // The head first: an unknown head is reported as such. A full sha costs no call.
    const { sha: head } = await forge.resolve(repo);
    if (spec.mergeBase) return { base: await forge.mergeBase(repo, spec.base, head), head };
    return { base: (await forge.resolve({ ...repo, ref: spec.base })).sha, head };
  }
  return forge.pull(repo, spec.n);
}

/** The last `n` commits reachable from the commit `sha`; cached, since the history of a commit never changes. */
export async function cachedCommits(forge: Forge, repo: Repo, sha: string, n: number, cache: TextCache): Promise<Commit[]> {
  const key = `commits:${forge.host}/${repo.owner}/${repo.name}@${sha}:${n}`;
  const kept = await cache.get(key);
  if (kept !== undefined) return JSON.parse(kept) as Commit[];
  const list = await forge.commits(repo, sha, n);
  await cache.put(key, JSON.stringify(list));
  return list;
}

export interface CommitRow {
  sha: string;
  short: string;
  title: string;
  author: string;
  /** YYYY-MM-DD of the author date, as the forge gives it. */
  day: string;
  /** The overlay shown is this commit against its first parent. */
  current: boolean;
}

/** The rows of the commit list; `shown` is the range of the overlay shown, if any. */
export function commitRows(list: Commit[], shown: Range | null): CommitRow[] {
  return list.map((c) => ({
    sha: c.sha,
    short: c.sha.slice(0, 7),
    title: c.title,
    author: c.author,
    day: c.date.slice(0, 10),
    current: shown !== null && shown.head === c.sha && shown.base === (c.parents[0] ?? null),
  }));
}

/** `diff.base` and `diff.head` of an overlay: short shas; `none` for the base of a root commit. */
export function overlayNames(range: Range): { base: string; head: string } {
  return { base: range.base === null ? 'none' : range.base.slice(0, 7), head: range.head.slice(0, 7) };
}
