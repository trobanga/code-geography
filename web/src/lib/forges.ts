// The kinds of forge (ADR 0022): which kind a host is, the `Forge` of a kind, and the parse of
// the input field and of the hash. The only place that maps a host or a kind to a provider.
import type { Fetch, Forge, ForgeKind } from './forge';
import type { Parsed } from './forgeDiff';
import { FORGEJO_FORMS, FORGEJO_OWNER, forgejo, forgejoUrlPath } from './forgejo';
import { github, parseRepo, withRange } from './github';

const KNOWN: Record<string, ForgeKind> = {
  'github.com': 'github',
  'www.github.com': 'github',
  'codeberg.org': 'forgejo',
  'gitlab.com': 'gitlab',
};

/**
 * The kind of a host: a known host, else Forgejo for now. The one place where a probe of the
 * host or a choice of the user would go.
 */
export function kindOf(host: string): ForgeKind {
  return KNOWN[host.toLowerCase()] ?? 'forgejo';
}

const GITLAB = 'GitLab is not supported yet';

/** The forge of a kind at `origin`, with the token of that origin; the provider decides how it sends the token. */
export function forgeFor(kind: ForgeKind, origin: string, net: { fetch: Fetch }, token?: string): Forge {
  if (kind === 'github') return github(net);
  if (kind === 'forgejo') return forgejo(net, origin, token);
  throw new Error(GITLAB);
}

const LOCAL = /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i;
const HOST = /^(?:[a-z0-9-]+\.)*[a-z0-9-]+(?::\d+)?$|^\[[0-9a-f:]+\](?::\d+)?$/i;
const FORMS = `give owner/repo or a github.com URL for GitHub, host/owner/repo[@ref | @A..B | @A...B], or a URL ${FORGEJO_FORMS}`;

/**
 * The text of the input field or of the hash (ADR 0020, 0022): `owner/repo[@ref]` or a
 * github.com URL for GitHub; a URL of another host; `host/path@ref`; or the hash form
 * `kind:host/path@ref`, which names its kind. A repository of a host other than GitHub carries
 * its origin and kind. Only `https:`, and `http:` for localhost.
 */
export function parseSource(input: string): Parsed {
  const s = input.trim();
  const bad = (why = ''): Parsed => ({ ok: false, error: `not a repository: "${s}"; ${why}${FORMS}` });
  const named = /^(github|forgejo|gitlab):(?![/\d])(.*)$/i.exec(s);
  const rest = named ? named[2] : s;
  const url = /^([a-z][a-z0-9+.-]*):\/\/([^/]+)\/(.*)$/i.exec(rest);
  const form = url ? null : /^([^/@]+)\/(.*)$/.exec(rest);
  if (!url && !(form && (named || /[.:]/.test(form[1]) || LOCAL.test(form[1])))) {
    const p = named ? null : parseRepo(s);
    return p?.ok ? p : bad();
  }
  const host = (url ? url[2] : form![1]).toLowerCase();
  const path = url ? url[3] : form![2];
  const kind = named ? (named[1].toLowerCase() as ForgeKind) : kindOf(host);
  if (kind === 'gitlab') return { ok: false, error: `${host}: ${GITLAB}` };
  if (kind === 'github') {
    const p = parseRepo(url ? rest : path);
    return p.ok ? p : bad();
  }
  const scheme = (url ? url[1] : LOCAL.test(host) ? 'http' : 'https').toLowerCase();
  if (!HOST.test(host)) return bad();
  if (scheme !== 'https' && !(scheme === 'http' && LOCAL.test(host))) return bad('only https, and http for localhost; ');
  const p = url ? forgejoUrlPath(path) : hostPath(path);
  return p?.ok ? { ...p, repo: { ...p.repo, origin: `${scheme}://${host}`, kind } } : bad();
}

/** `owner/name[@ref]`, split at the last `/`: the kind, not the parse, says how many segments an owner may have. */
function hostPath(path: string): Parsed | null {
  const at = path.indexOf('@');
  const repo = at < 0 ? path : path.slice(0, at);
  const slash = repo.lastIndexOf('/');
  if (slash < 0) return null;
  return withRange(repo.slice(0, slash), repo.slice(slash + 1), at < 0 ? undefined : path.slice(at + 1), FORGEJO_OWNER);
}
