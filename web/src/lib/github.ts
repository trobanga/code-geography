// Opening a public GitHub repository in the browser (ADR 0020): tree API, raw host, cache by blob sha.
import { inputError } from './project';

export interface Repo {
  owner: string;
  name: string;
  /** As typed: a branch, a tag or a commit sha. Absent: the default branch. */
  ref?: string;
}

export type Parsed = { ok: true; repo: Repo; note?: string } | { ok: false; error: string };

const OWNER = /^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?$/;
const NAME = /^[A-Za-z0-9._-]+$/;
// What git allows in a ref name, roughly: no space, no control character, none of ~^:?*[\ .
const REF = /^[^\s~^:?*[\\]+$/;

const FORMS = 'give owner/repo, owner/repo@ref, or a URL https://github.com/owner/repo[/tree/<ref> | /commit/<sha>]';

function make(owner: string, name: string, ref: string | undefined): Repo | null {
  if (!OWNER.test(owner) || !NAME.test(name) || name === '.' || name === '..') return null;
  if (ref !== undefined && !REF.test(ref)) return null;
  return ref === undefined ? { owner, name } : { owner, name, ref };
}

/** The repository and ref of the text in the input field (G1), or why it is not one. */
export function parseRepo(input: string): Parsed {
  const s = input.trim();
  const bad: Parsed = { ok: false, error: `not a GitHub repository: "${s}"; ${FORMS}` };
  const url = /^(?:https?:\/\/)?(?:www\.)?github\.com\/(.*)$/i.exec(s);
  if (url) {
    const [owner = '', rawName = '', kind, ...rest] = url[1].replace(/\/$/, '').split('/');
    if (kind !== undefined && kind !== 'tree' && kind !== 'commit') return bad;
    const repo = make(owner, rawName.replace(/\.git$/, ''), kind === undefined ? undefined : rest.join('/'));
    if (!repo) return bad;
    // A URL cannot tell a branch `feature/x` from a directory `x` of branch `feature`.
    if (kind === 'tree' && rest.length > 1)
      return { ok: true, repo, note: `took "${repo.ref}" as the whole branch name; a path inside a branch is not supported` };
    return { ok: true, repo };
  }
  const at = s.indexOf('@');
  const parts = (at < 0 ? s : s.slice(0, at)).split('/');
  if (parts.length !== 2) return bad;
  const repo = make(parts[0], parts[1], at < 0 ? undefined : s.slice(at + 1));
  return repo ? { ok: true, repo } : bad;
}

/** The network as `fetch` gives it; a parameter, so that tests and S3 can pass their own. */
export type Fetch = (url: string, init?: { headers?: Record<string, string> }) => Promise<Response>;

export interface Net {
  fetch: Fetch;
  /** For private repositories (S3); unused for now. */
  token?: string;
}

const API = 'https://api.github.com';

/** The commit sha of the ref (default branch if absent): one API call. */
export async function resolve(repo: Repo, net: Net): Promise<{ sha: string }> {
  if (repo.ref && /^[0-9a-f]{40}$/i.test(repo.ref)) return { sha: repo.ref.toLowerCase() };
  const url = `${API}/repos/${repo.owner}/${repo.name}/commits/${encodeURIComponent(repo.ref ?? 'HEAD')}`;
  const r = await get(net, url, repo, { Accept: 'application/vnd.github.sha' });
  return { sha: (await r.text()).trim() };
}

/** One request; a failed one rejects with the message of `httpError` or of the network. */
async function get(net: Net, url: string, repo: Repo, headers: Record<string, string> = {}): Promise<Response> {
  let r: Response;
  try {
    r = await net.fetch(url, { headers });
  } catch (e) {
    throw new Error(`cannot reach ${new URL(url).host} (network failure): ${e instanceof Error ? e.message : String(e)}`);
  }
  if (!r.ok) throw new Error(httpError(r, repo));
  return r;
}

/** The message for a failed answer of GitHub. */
export function httpError(r: Response, repo: Repo): string {
  const id = `${repo.owner}/${repo.name}`;
  if (r.status === 404) return `${id}: not found, or private (a private repository needs a token, not supported yet)`;
  if ((r.status === 403 || r.status === 429) && r.headers.get('x-ratelimit-remaining') === '0') {
    const reset = Number(r.headers.get('x-ratelimit-reset'));
    const after = Number.isFinite(reset) && reset > 0 ? `; try again after ${new Date(reset * 1000).toLocaleTimeString()}` : '';
    return `GitHub rate limit reached (60 requests per hour without a token)${after}`;
  }
  if (r.status === 422 && repo.ref) return `${id}: no branch, tag or commit "${repo.ref}"`;
  return `${id}: HTTP ${r.status}`;
}

export interface Entry {
  path: string;
  blobSha: string;
}

/** The files of a commit with their blob shas: one API call, none when the cache has the tree of this commit. */
export async function tree(repo: Repo, sha: string, net: Net, cache: TextCache = memoryCache()): Promise<Entry[]> {
  // A commit sha names its tree, so the key never goes stale; it cannot clash with a blob sha.
  const key = `tree:${repo.owner}/${repo.name}@${sha}`;
  const kept = await cache.get(key);
  if (kept !== undefined) return JSON.parse(kept) as Entry[];
  const url = `${API}/repos/${repo.owner}/${repo.name}/git/trees/${sha}?recursive=1`;
  const body = (await (await get(net, url, repo)).json()) as { truncated: boolean; tree: { path: string; type: string; sha: string }[] };
  // A part of the tree would give a graph that looks whole and is not.
  if (body.truncated)
    throw new Error(`${repo.owner}/${repo.name}: too large; GitHub truncated the file list (about 100,000 entries or 7 MB), and a part would give a wrong graph`);
  const entries = body.tree.filter((e) => e.type === 'blob').map((e) => ({ path: e.path, blobSha: e.sha }));
  await cache.put(key, JSON.stringify(entries));
  return entries;
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
  get(blobSha: string): Promise<string | undefined>;
  put(blobSha: string, text: string): Promise<void>;
}

export function memoryCache(): TextCache {
  const m = new Map<string, string>();
  return {
    get: async (k) => m.get(k),
    put: async (k, v) => void m.set(k, v),
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
  };
}

const RAW = 'https://raw.githubusercontent.com';
/** Requests to the raw host at a time. */
export const PARALLEL = 10;

/** The texts of the entries at commit `sha`, from the cache or else the raw host, which does not count against the API limit. */
export async function texts(
  repo: Repo,
  sha: string,
  entries: Entry[],
  net: Net,
  cache: TextCache,
  onProgress: (done: number, all: number) => void = () => {},
): Promise<{ path: string; text: string }[]> {
  let done = 0;
  return pool(entries, PARALLEL, async (e) => {
    let text = await cache.get(e.blobSha);
    if (text === undefined) {
      const path = e.path.split('/').map(encodeURIComponent).join('/');
      text = await (await get(net, `${RAW}/${repo.owner}/${repo.name}/${sha}/${path}`, repo)).text();
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

/** The inputs of the indexer at the ref: at most two API calls (G2), none for a cached commit given by sha. */
export async function fetchRepo(
  repo: Repo,
  net: Net,
  cache: TextCache,
  isInput: (path: string) => boolean,
  onPhase: (phase: string) => void = () => {},
): Promise<Fetched> {
  const id = `${repo.owner}/${repo.name}`;
  onPhase(`resolving ${id}${repo.ref ? '@' + repo.ref : ''}`);
  const { sha } = await resolve(repo, net);
  onPhase(`listing the files of ${id}@${sha.slice(0, 7)}`);
  const inputs = inputsOf(await tree(repo, sha, net, cache), isInput);
  const err = inputError({ root: id, inputs: inputs.map((e) => ({ path: e.path, file: e })) });
  if (err) throw new Error(err);
  const got = await texts(repo, sha, inputs, net, cache, (done, all) => onPhase(`fetched ${done} / ${all} files of ${id}`));
  return { sha, root: repo.name, paths: got.map((g) => g.path), texts: got.map((g) => g.text) };
}
