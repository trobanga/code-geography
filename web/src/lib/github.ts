// GitHub as a `Forge` (ADR 0020): the only place that knows GitHub URLs and answers.
// The tree comes from the API (one call), the texts from the raw host (no API limit).
import type { Branch, Commit, Entry, Fetch, Forge, Repo } from './forge';
import type { Parsed } from './forgeDiff';

const OWNER = /^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?$/;
const NAME = /^[A-Za-z0-9._-]+$/;
// What git allows in a ref name, roughly: no space, no control character, none of ~^:?*[\ .
const REF = /^[^\s~^:?*[\\]+$/;

const FORMS =
  'give owner/repo, owner/repo@ref, owner/repo@A..B, owner/repo@A...B, or a URL https://github.com/owner/repo[/tree/<ref> | /commit/<sha> | /compare/A...B | /pull/<n>]';

function make(owner: string, name: string, ref: string | undefined, ownerRe = OWNER): Repo | null {
  if (!ownerRe.test(owner) || !NAME.test(name) || name === '.' || name === '..') return null;
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
    if (kind === 'pull') {
      const n = /^[1-9][0-9]*$/.test(rest[0] ?? '') ? Number(rest[0]) : NaN;
      const repo = make(owner, rawName.replace(/\.git$/, ''), undefined);
      return repo && Number.isSafeInteger(n) ? { ok: true, repo, overlay: { kind: 'pull', n } } : bad;
    }
    if (kind === 'compare') {
      const range = rest.join('/');
      return (range.includes('..') && withRange(owner, rawName.replace(/\.git$/, ''), range)) || bad;
    }
    if (kind !== undefined && kind !== 'tree' && kind !== 'commit') return bad;
    const repo = make(owner, rawName.replace(/\.git$/, ''), kind === undefined ? undefined : rest.join('/'));
    if (!repo) return bad;
    // A URL cannot tell a branch `feature/x` from a directory `x` of branch `feature`.
    if (kind === 'tree' && rest.length > 1)
      return { ok: true, repo, note: `took "${repo.ref}" as the whole branch name; a path inside a branch is not supported` };
    return kind === 'commit' ? { ok: true, repo, overlay: { kind: 'commit' } } : { ok: true, repo };
  }
  const at = s.indexOf('@');
  const parts = (at < 0 ? s : s.slice(0, at)).split('/');
  if (parts.length !== 2) return bad;
  return withRange(parts[0], parts[1], at < 0 ? undefined : s.slice(at + 1)) ?? bad;
}

/**
 * The repository at `ref`; a ref `A..B` or `A...B` is the overlay of a range (git forbids `..`
 * in a ref name). `ownerRe`: the owner names of the host; GitHub's by default.
 */
export function withRange(owner: string, name: string, ref: string | undefined, ownerRe = OWNER): Parsed | null {
  const dots = ref === undefined ? -1 : ref.indexOf('..');
  if (ref === undefined || dots < 0) {
    const repo = make(owner, name, ref, ownerRe);
    return repo && { ok: true, repo };
  }
  const mergeBase = ref.startsWith('...', dots);
  const base = ref.slice(0, dots);
  const repo = make(owner, name, ref.slice(dots + (mergeBase ? 3 : 2)), ownerRe);
  if (!repo || !REF.test(base) || base.includes('..') || repo.ref!.includes('..')) return null;
  return { ok: true, repo, overlay: { kind: 'range', base, mergeBase } };
}

export interface Net {
  fetch: Fetch;
  /** For private repositories, later; sent to the hosts of GitHub only. Unused for now. */
  token?: string;
}

const API = 'https://api.github.com';
const RAW = 'https://raw.githubusercontent.com';

/** GitHub with the given network. */
export function github(net: Net): Forge {
  return {
    host: 'github.com',
    name: 'GitHub',

    /** One API call; none for a full sha. */
    async resolve(repo) {
      if (repo.ref && /^[0-9a-f]{40}$/i.test(repo.ref)) return { sha: repo.ref.toLowerCase() };
      const url = `${API}/repos/${repo.owner}/${repo.name}/commits/${encodeURIComponent(repo.ref ?? 'HEAD')}`;
      const r = await get(net, url, repo, { Accept: 'application/vnd.github.sha' });
      return { sha: (await r.text()).trim() };
    },

    /** One API call. */
    async tree(repo, sha) {
      const url = `${API}/repos/${repo.owner}/${repo.name}/git/trees/${sha}?recursive=1`;
      const body = (await (await get(net, url, repo)).json()) as { truncated: boolean; tree: { path: string; type: string; sha: string }[] };
      // A part of the tree would give a graph that looks whole and is not.
      if (body.truncated)
        throw new Error(`${repo.owner}/${repo.name}: too large; GitHub truncated the file list (about 100,000 entries or 7 MB), and a part would give a wrong graph`);
      return body.tree.filter((e) => e.type === 'blob').map((e): Entry => ({ path: e.path, blobSha: e.sha }));
    },

    /** From the raw host, which does not count against the API limit. */
    async text(repo, sha, path) {
      const p = path.split('/').map(encodeURIComponent).join('/');
      return (await get(net, `${RAW}/${repo.owner}/${repo.name}/${sha}/${p}`, repo)).text();
    },

    /** One API call. */
    async commits(repo, sha, n, page = 1) {
      const url = `${API}/repos/${repo.owner}/${repo.name}/commits?sha=${encodeURIComponent(sha)}&per_page=${n}${page > 1 ? `&page=${page}` : ''}`;
      const body = (await (await get(net, url, { ...repo, ref: sha })).json()) as {
        sha: string;
        parents: { sha: string }[];
        commit: { message: string; author: { name: string; date: string } | null };
      }[];
      return body.map(
        (c): Commit => ({
          sha: c.sha,
          parents: c.parents.map((p) => p.sha),
          title: c.commit.message.split('\n', 1)[0],
          author: c.commit.author?.name ?? '',
          date: c.commit.author?.date ?? '',
        }),
      );
    },

    /** One API call. */
    async mergeBase(repo, a, b) {
      return mergeBase(net, repo, a, b);
    },

    /** Two API calls: the pull request gives the tip of its base branch, not the merge base. */
    async pull(repo, n) {
      const url = `${API}/repos/${repo.owner}/${repo.name}/pulls/${n}`;
      let r: Response;
      try {
        r = await get(net, url, repo);
      } catch (e) {
        if (e instanceof NotFound) throw new Error(`${repo.owner}/${repo.name}: no pull request #${n} (or the repository is private)`);
        throw e;
      }
      const body = (await r.json()) as { head: { sha: string }; base: { sha: string } };
      return { base: await mergeBase(net, repo, body.base.sha, body.head.sha), head: body.head.sha };
    },

    /** One API call; the first page of 100. */
    async branches(repo) {
      const url = `${API}/repos/${repo.owner}/${repo.name}/branches?per_page=100`;
      const body = (await (await get(net, url, repo)).json()) as { name: string; commit: { sha: string } }[];
      return body.map((b): Branch => ({ name: b.name, sha: b.commit.sha }));
    },
  };
}

async function mergeBase(net: Net, repo: Repo, a: string, b: string): Promise<string> {
  const range = `${encodeURIComponent(a)}...${encodeURIComponent(b)}`;
  const url = `${API}/repos/${repo.owner}/${repo.name}/compare/${range}?per_page=1`;
  let r: Response;
  try {
    r = await get(net, url, repo);
  } catch (e) {
    if (e instanceof NotFound)
      throw new Error(`${repo.owner}/${repo.name}: cannot compare ${a}...${b}: an unknown branch, tag or commit, or no common history`);
    throw e;
  }
  return ((await r.json()) as { merge_base_commit: { sha: string } }).merge_base_commit.sha;
}

/** A 404 answer, which some callers explain better than `httpError` can. */
class NotFound extends Error {}

/** One request; a failed one rejects with the message of `httpError` or of the network. */
async function get(net: Net, url: string, repo: Repo, headers: Record<string, string> = {}): Promise<Response> {
  let r: Response;
  try {
    r = await net.fetch(url, { headers });
  } catch (e) {
    throw new Error(`cannot reach ${new URL(url).host} (network failure): ${e instanceof Error ? e.message : String(e)}`);
  }
  if (r.status === 404) throw new NotFound(httpError(r, repo));
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
