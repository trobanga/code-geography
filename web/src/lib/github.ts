// GitHub as a `Forge` (ADR 0020): the only place that knows GitHub URLs and answers.
// The tree comes from the API (one call), the texts from the raw host (no API limit).
import type { Entry, Fetch, Forge, Repo } from './forge';

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
  };
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
