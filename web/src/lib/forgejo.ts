// Forgejo (and Gitea) as a `Forge` (ADR 0022): the only place that knows their URLs and answers.
// Every URL is built from the origin and a path; no URL of an answer is ever followed (T-2).
import type { Branch, Commit, Entry, Fetch, Forge, Repo } from './forge';
import type { Parsed } from './forgeDiff';
import { withRange } from './github';
import { authorized, ForeignOrigin } from './tokenStore';

const FULL = /^[0-9a-f]{40}$/i;
const LEAN = 'stat=false&verification=false&files=false';

interface TreePage {
  tree: { path: string; type: string; sha: string }[];
  total_count: number;
}

/** A Forgejo host at `origin` (`https://codeberg.org`), with the token of that origin, if any. */
export function forgejo(net: { fetch: Fetch }, origin: string, token?: string): Forge {
  const send = authorized(net.fetch, origin, token === undefined ? undefined : { Authorization: `token ${token}` });
  const api = (repo: Repo, path: string) => `${origin}/api/v1/repos/${encodeURIComponent(repo.owner)}/${encodeURIComponent(repo.name)}${path}`;
  const host = new URL(origin).host;
  /** One request; a failed one rejects with a message for the user that never holds the answer, which may echo the token. */
  async function get(url: string, repo: Repo, ref?: string): Promise<Response> {
    let r: Response;
    try {
      r = await send(url);
    } catch (e) {
      if (e instanceof ForeignOrigin) throw e;
      // The browser hides the reason; a host that does not allow cross-origin reads is the common one.
      const viewer = globalThis.location?.origin ?? 'the origin of this viewer';
      throw new Error(
        `cannot reach ${host}: the network failed, or, more probably, ${host} does not allow requests from this page (CORS); its admin must set [cors] ENABLED = true and ALLOW_DOMAIN = ${viewer} in app.ini`,
      );
    }
    if (r.ok) return r;
    const id = `${repo.owner}/${repo.name}`;
    if (r.status === 401) throw new Error(token ? `${host}: the token is wrong or expired (HTTP 401)` : `${host} asks for a token (HTTP 401)`);
    if (r.status === 403)
      throw new Error(token ? `${host}: the token may not read ${id}; it needs the scope read:repository (HTTP 403)` : `${host} refused to read ${id} (HTTP 403)`);
    if (r.status === 404) {
      const what = ref ? `no branch, tag or commit "${ref}", or the repository is not found` : 'not found';
      throw new Error(`${id}: ${what}${token ? `, or the token of ${host} has no access to it` : ' or private: give a token'}`);
    }
    if (r.status === 429) throw new Error(`${host}: too many requests (HTTP 429); try again in a few minutes`);
    throw new Error(`${id}: HTTP ${r.status}`);
  }
  /** One API call; none for a full sha. */
  async function resolve(repo: Repo): Promise<{ sha: string }> {
    if (repo.ref && FULL.test(repo.ref)) return { sha: repo.ref.toLowerCase() };
    const r = await get(api(repo, `/git/commits/${encodeURIComponent(repo.ref ?? 'HEAD')}?${LEAN}`), repo, repo.ref);
    return { sha: ((await r.json()) as { sha: string }).sha };
  }

  /** The commits that compare `a...b` lists; rejects rather than give a part. */
  async function compare(repo: Repo, a: string, b: string) {
    const url = api(repo, `/compare/${encodeURIComponent(a)}...${encodeURIComponent(b)}?files=false&verification=false`);
    const body = (await (await get(url, repo, `${a}...${b}`)).json()) as { commits: { sha: string; parents: { sha: string }[] | null }[] | null; total_commits: number };
    const commits = (body.commits ?? []).map((c) => ({ sha: c.sha, parents: c.parents ?? [] }));
    if (commits.length !== body.total_commits)
      throw new Error(`${repo.owner}/${repo.name}: the compare of ${a}...${b} listed ${commits.length} of ${body.total_commits} commits; cannot find the merge base`);
    return commits;
  }
  return {
    host,
    name: host,

    resolve,
    /** One API call per 1000 entries: the tree API is paged. Rejects rather than give a part. */
    async tree(repo, sha) {
      const out: Entry[] = [];
      let seen = 0;
      for (let page = 1; ; page++) {
        const body = (await (await get(api(repo, `/git/trees/${sha}?recursive=true&page=${page}`), repo)).json()) as TreePage;
        seen += body.tree.length;
        for (const e of body.tree) if (e.type === 'blob') out.push({ path: e.path, blobSha: e.sha });
        if (seen === body.total_count) return out;
        // An empty page before the end, or more than announced: the tree changed or the paging broke.
        if (body.tree.length === 0 || seen > body.total_count)
          throw new Error(`${repo.owner}/${repo.name}: the forge listed ${seen} of ${body.total_count} entries of the tree; a part would give a wrong graph`);
      }
    },
    /** One API call: the raw route of the API, which allows CORS; the web route `/raw/commit/` does not. */
    async text(repo, sha, path) {
      const p = path.split('/').map(encodeURIComponent).join('/');
      return (await get(api(repo, `/raw/${p}?ref=${sha}`), repo)).text();
    },
    /** One API call. */
    async commits(repo, sha, n, page = 1) {
      const more = page > 1 ? `&page=${page}` : '';
      const body = (await (await get(api(repo, `/commits?sha=${encodeURIComponent(sha)}&limit=${n}&${LEAN}${more}`), repo, sha)).json()) as {
        sha: string;
        parents: { sha: string }[] | null;
        commit: { message: string; author: { name: string; date: string } | null };
      }[];
      return body.map(
        (c): Commit => ({
          sha: c.sha,
          parents: (c.parents ?? []).map((p) => p.sha),
          title: c.commit.message.split('\n', 1)[0],
          author: c.commit.author?.name ?? '',
          date: c.commit.author?.date ?? '',
        }),
      );
    },

    /**
     * One API call, plus one per further candidate. Forgejo's compare `a...b` lists the commits
     * from the merge base M to b without M, and not M itself. Each parent of a listed commit
     * that is not listed is M or an ancestor of M, and M is one of them; of two candidates x, y,
     * `x...y` lists nothing when y is an ancestor of x.
     */
    async mergeBase(repo, a, b) {
      const listed = await compare(repo, a, b);
      if (listed.length === 0) return (await resolve({ ...repo, ref: b })).sha;
      const inside = new Set(listed.map((c) => c.sha));
      const outside = [...new Set(listed.flatMap((c) => c.parents.map((p) => p.sha)).filter((p) => !inside.has(p)))];
      let best = outside[0];
      for (const y of outside.slice(1)) if ((await compare(repo, best, y)).length > 0) best = y;
      return best;
    },

    /** One API call: a Forgejo pull request carries its merge base. */
    async pull(repo, n) {
      const body = (await (await get(api(repo, `/pulls/${n}`), repo)).json()) as { merge_base: string; head: { sha: string } };
      return { base: body.merge_base, head: body.head.sha };
    },

    /** One API call; the first page of 100. */
    async branches(repo) {
      const body = (await (await get(api(repo, '/branches?limit=100'), repo)).json()) as { name: string; commit: { id: string } }[];
      return body.map((b): Branch => ({ name: b.name, sha: b.commit.id }));
    },
  };
}

// Forgejo user and organization names: letters, digits, `-`, `_`, `.`; one segment.
export const FORGEJO_OWNER = /^[A-Za-z0-9_.-]+$/;

/** The forms of the input that a Forgejo host takes, for the message of a bad input. */
export const FORGEJO_FORMS = 'https://<host>/owner/repo[/src/branch/<b> | /src/tag/<t> | /src/commit/<sha> | /commit/<sha> | /compare/A...B | /pulls/<n>]';

/** The path of a Forgejo web URL after the host: the repository, maybe a ref and an overlay; null if it is none. */
export function forgejoUrlPath(path: string): Parsed | null {
  const OWNER = FORGEJO_OWNER;
  const [owner = '', raw = '', kind, ...rest] = path.replace(/\/$/, '').split('/');
  const name = raw.replace(/\.git$/, '');
  if (kind === undefined) return withRange(owner, name, undefined, OWNER);
  if (kind === 'pulls') {
    const n = /^[1-9][0-9]*$/.test(rest[0] ?? '') ? Number(rest[0]) : NaN;
    const p = withRange(owner, name, undefined, OWNER);
    return p?.ok && Number.isSafeInteger(n) ? { ...p, overlay: { kind: 'pull', n } } : null;
  }
  if (kind === 'compare') {
    const range = rest.join('/');
    return range.includes('..') ? withRange(owner, name, range, OWNER) : null;
  }
  if (kind === 'commit') {
    const p = rest.length === 1 ? withRange(owner, name, rest[0], OWNER) : null;
    return p?.ok ? { ...p, overlay: { kind: 'commit' } } : null;
  }
  if (kind === 'src' && ['branch', 'tag', 'commit'].includes(rest[0]) && rest.length > 1) {
    const ref = rest.slice(1).join('/');
    const p = withRange(owner, name, ref, OWNER);
    if (!p?.ok || p.overlay) return null;
    // A URL cannot tell a branch `feature/x` from a directory `x` of branch `feature`.
    return rest.length > 2 ? { ...p, note: `took "${ref}" as the whole ${rest[0]} name; a path inside it is not supported` } : p;
  }
  return null;
}
