// Opening a repository of a forge in the browser (ADR 0020): the parts that know no forge.
// A `Forge` knows the URLs and the answers of one host; this module pools the requests,
// caches by blob sha, reports progress and hands the inputs to the indexer.
import { inputError } from './project';

export interface Repo {
  owner: string;
  name: string;
  /** As typed: a branch, a tag or a commit sha. Absent: the default branch. */
  ref?: string;
  /** The forge host, e.g. `https://codeberg.org` (ADR 0022). Absent: GitHub. */
  origin?: string;
  /** The kind of the forge at `origin` (ADR 0022); with `origin`. */
  kind?: ForgeKind;
}

/** The kinds of forge. `gitlab` is planned and not built (ADR 0022). */
export type ForgeKind = 'github' | 'forgejo' | 'gitlab';

/** One commit of a history list. */
export interface Commit {
  sha: string;
  /** In order; the first is the one an overlay compares with. Empty for a root commit. */
  parents: string[];
  /** The first line of the message. */
  title: string;
  author: string;
  /** ISO 8601, as the forge gives it. */
  date: string;
  /** The full message, when the answer held it; a list cached before CG-4 lacks it. */
  message?: string;
}

/** A commit with its full message. */
export interface CommitText extends Commit {
  message: string;
}

export type PullState = 'open' | 'closed' | 'merged';

export interface PullRequest {
  number: number;
  title: string;
  state: PullState;
  url: string;
  body: string;
}

/** What the commit context needs of a forge (CG-1). */
export interface CommitSource {
  /** The commits of `base..head` (reachable from head, not from base), newest first, with full messages; one call. `base` null: the head alone. `total`: how many the range holds; `commits` may hold fewer. */
  between(repo: Repo, base: string | null, head: string): Promise<{ commits: CommitText[]; total: number }>;
  /** The pull requests the forge links to the commit; [] for none. One call. Their `url` is built by the forge, not taken from the answer. */
  pullsOf(repo: Repo, sha: string): Promise<PullRequest[]>;
  /** The web pages of the repository on its host; no call. */
  links(repo: Repo): WebLinks;
}

/**
 * The web pages of one repository, built from the origin, the names and the numbers; never
 * taken from an answer (ADR 0022, T-2). Only a forge knows its URL forms (ADR 0020).
 */
export interface WebLinks {
  commit(sha: string): string;
  pull(n: number): string;
  /** An issue of `owner/name` on the same host. */
  issue(owner: string, name: string, n: number): string;
  /** The issue or pull request that a URL of this host names, as a text may hold it; null for any other URL. */
  reference(url: string): { owner: string; name: string; n: number } | null;
}

export interface Entry {
  path: string;
  blobSha: string;
}

/** The network as `fetch` gives it; a parameter, so that tests and later steps can pass their own. */
export type Fetch = (url: string, init?: { headers?: Record<string, string>; redirect?: RequestRedirect }) => Promise<Response>;

/**
 * One host with its API. All of its requests carry its own token, if any, so a token never
 * leaves the host it belongs to. Each method rejects with a message for the user.
 */
export interface Forge {
  /** E.g. `github.com`; part of the cache key of a tree. */
  host: string;
  /** The name for the user, e.g. `GitHub` or `codeberg.org`. */
  name: string;
  /** The commit sha of the ref, of the default branch when the ref is absent. */
  resolve(repo: Repo): Promise<{ sha: string }>;
  /** All files of the commit with their blob shas; rejects rather than give a part. */
  tree(repo: Repo, sha: string): Promise<Entry[]>;
  /** The text of one file at the commit. */
  text(repo: Repo, sha: string, path: string): Promise<string>;
  /**
   * The last `n` commits reachable from the commit `sha` (it first), newest first. A ref works too.
   * `page` (from 1) gives the next `n` commits each; a page with less than `n` is the last one.
   */
  commits(repo: Repo, sha: string, n: number, page?: number): Promise<Commit[]>;
  /** The commit sha of the merge base of two refs (`git merge-base a b`). */
  mergeBase(repo: Repo, a: string, b: string): Promise<string>;
  /** The head commit of a pull request and its merge base with the base branch. */
  pull(repo: Repo, n: number): Promise<{ base: string; head: string }>;
  /** The name of the default branch of the repository: the branch of `HEAD`. */
  defaultBranch(repo: Repo): Promise<string>;
  /** All branches of the repository, in the order of the forge; one call per page. */
  branches(repo: Repo): Promise<Branch[]>;
}

/** A branch with the sha of its head commit. */
export interface Branch {
  name: string;
  sha: string;
}

/** `work` on each item with at most `limit` running at a time; the results in the order of the items. Rejects on the first failure. */
export async function pool<T, R>(items: T[], limit: number, work: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  let failed = false;
  async function lane() {
    while (!failed && next < items.length) {
      const i = next++;
      try {
        out[i] = await work(items[i]);
      } catch (e) {
        failed = true;
        throw e;
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, lane));
  return out;
}

/** File texts by blob sha (G4). A blob sha names its content, so an entry never goes stale. */
export interface TextCache {
  get(key: string): Promise<string | undefined>;
  put(key: string, text: string): Promise<void>;
  /** Removes every entry, of all hosts: a text by blob sha does not know its host. */
  clear(): Promise<void>;
}

export function memoryCache(): TextCache {
  const m = new Map<string, string>();
  return {
    get: async (k) => m.get(k),
    put: async (k, v) => void m.set(k, v),
    clear: async () => m.clear(),
  };
}

const DB = 'code-geography';
const STORE = 'blobs';

function req<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((ok, err) => {
    r.onsuccess = () => ok(r.result);
    r.onerror = () => err(r.error);
  });
}

/**
 * The cache in IndexedDB, which outlives a reload; in memory when IndexedDB is missing or
 * refuses (private window, blocked site data). A failed read or write later counts as a miss.
 */
export async function openCache(factory: IDBFactory | undefined = globalThis.indexedDB): Promise<TextCache> {
  let db: IDBDatabase;
  try {
    if (!factory) return memoryCache();
    const open = factory.open(DB, 1);
    open.onupgradeneeded = () => open.result.createObjectStore(STORE);
    db = await req(open);
  } catch {
    return memoryCache();
  }
  return {
    async get(k) {
      try {
        const v = await req(db.transaction(STORE).objectStore(STORE).get(k));
        return typeof v === 'string' ? v : undefined;
      } catch {
        return undefined;
      }
    },
    async put(k, v) {
      try {
        await req(db.transaction(STORE, 'readwrite').objectStore(STORE).put(v, k));
      } catch {
        // Full or blocked: the next open fetches the file again.
      }
    },
    async clear() {
      await req(db.transaction(STORE, 'readwrite').objectStore(STORE).clear());
    },
  };
}

/** File requests at a time. */
export const PARALLEL = 10;

/** The files of a commit, from the cache or else from the forge. A commit sha names its tree, so the entry never goes stale. */
export async function cachedTree(forge: Forge, repo: Repo, sha: string, cache: TextCache): Promise<Entry[]> {
  // The prefix keeps it apart from the blob shas, the host apart from another forge.
  const key = `tree:${forge.host}/${repo.owner}/${repo.name}@${sha}`;
  const kept = await cache.get(key);
  if (kept !== undefined) return JSON.parse(kept) as Entry[];
  const entries = await forge.tree(repo, sha);
  await cache.put(key, JSON.stringify(entries));
  return entries;
}

/** The texts of the entries at commit `sha`, from the cache or else from the forge. */
export async function texts(
  forge: Forge,
  repo: Repo,
  sha: string,
  entries: Entry[],
  cache: TextCache,
  onProgress: (done: number, all: number) => void = () => {},
): Promise<{ path: string; text: string }[]> {
  let done = 0;
  return pool(entries, PARALLEL, async (e) => {
    let text = await cache.get(e.blobSha);
    if (text === undefined) {
      text = await forge.text(repo, sha, e.path);
      await cache.put(e.blobSha, text);
    }
    onProgress(++done, entries.length);
    return { path: e.path, text };
  });
}

/** The entries that the indexer reads (`isInput` of the WASM module), sorted by path as the walk sorts them. */
export function inputsOf(entries: Entry[], isInput: (path: string) => boolean): Entry[] {
  return entries.filter((e) => isInput(e.path)).sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

export interface Fetched {
  sha: string;
  /** The repository name: the root name of the graph. */
  root: string;
  paths: string[];
  texts: string[];
}

/** The inputs of the indexer at the ref: one `resolve`, one `tree` (none for a cached commit), then the files. */
export async function fetchRepo(
  forge: Forge,
  repo: Repo,
  cache: TextCache,
  isInput: (path: string) => boolean,
  onPhase: (phase: string) => void = () => {},
): Promise<Fetched> {
  const id = `${repo.owner}/${repo.name}`;
  onPhase(`resolving ${id}${repo.ref ? '@' + repo.ref : ''}`);
  const { sha } = await forge.resolve(repo);
  onPhase(`listing the files of ${id}@${sha.slice(0, 7)}`);
  const inputs = inputsOf(await cachedTree(forge, repo, sha, cache), isInput);
  const err = inputError({ root: id, inputs: inputs.map((e) => ({ path: e.path, file: e })) });
  if (err) throw new Error(err);
  const got = await texts(forge, repo, sha, inputs, cache, (done, all) => onPhase(`fetched ${done} / ${all} files of ${id}`));
  return { sha, root: repo.name, paths: got.map((g) => g.path), texts: got.map((g) => g.text) };
}
