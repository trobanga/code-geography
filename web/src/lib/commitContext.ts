// The commit context (ADR 0030): the full message of the commits of an overlay, their pull
// requests and the issue references that the texts name, on demand. Pure but for the
// `CommitSource` that a caller passes in.
import type { CommitSource, CommitText, PullRequest, Repo, WebLinks } from './forge';
import type { Range } from './forgeDiff';

export interface IssueRef {
  owner: string;
  name: string;
  n: number;
  /** '#12' in the same repository, 'o/r#12' else */
  label: string;
  url: string;
}

/** A URL in a text, any scheme with `//`; the characters that commonly close a sentence or a bracket end it. */
const URL_OR_REF = /(?<url>[a-z][a-z0-9+.-]*:\/\/[^\s<>()[\]"'`]+)|(?<![\w/&.])(?:(?<ro>[\w.-]+)\/(?<rr>[\w.-]+))?#(?<rn>\d+)(?!\w)/gi;

/** What the forge reads from a URL without its query, fragment and closing punctuation. */
const referenceIn = (url: string, links: WebLinks) => links.reference(url.replace(/[?#].*$/, '').replace(/[.,;:!?]+$/, ''));

/**
 * References `#12` and `owner/repo#12` in the text, and the URLs of an issue or pull request
 * that the forge reads as its own, in order of first appearance, without duplicates. The forge
 * builds the links. A reference is a guess: it is read from the text, not checked (ADR 0030).
 */
export function issueRefs(text: string, repo: Repo, links: WebLinks): IssueRef[] {
  const out: IssueRef[] = [];
  const seen = new Set<string>();
  for (const m of text.matchAll(URL_OR_REF)) {
    const g = m.groups!;
    const found = g.url !== undefined ? referenceIn(g.url, links) : { owner: g.ro ?? repo.owner, name: g.rr ?? repo.name, n: Number(g.rn) };
    if (!found) continue;
    const { owner, name, n } = found;
    const key = `${owner}/${name}#${n}`.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    const same = owner.toLowerCase() === repo.owner.toLowerCase() && name.toLowerCase() === repo.name.toLowerCase();
    out.push({ owner, name, n, label: same ? `#${n}` : `${owner}/${name}#${n}`, url: links.issue(owner, name, n) });
  }
  return out;
}

export interface CommitView {
  sha: string;
  short: string;
  title: string;
  body: string;
  /** The page of the commit on its host. */
  url: string;
  pulls: (PullRequest & { label: string })[];
  refs: IssueRef[];
}

/**
 * The parts of a commit for display, the links from the forge. refs: from the message and all
 * PR bodies, minus those that name a listed PR of the same repository.
 */
export function commitView(links: WebLinks, repo: Repo, c: CommitText, pulls: PullRequest[]): CommitView {
  const listed = new Set(pulls.map((p) => p.number));
  const refs = issueRefs([c.message, ...pulls.map((p) => p.body)].join('\n'), repo, links).filter(
    (r) => !(r.label === `#${r.n}` && listed.has(r.n)),
  );
  return {
    sha: c.sha,
    short: c.sha.slice(0, 7),
    title: c.title,
    body: commitBody(c.message),
    url: links.commit(c.sha),
    pulls: pulls.map((p) => ({ ...p, label: `#${p.number}` })),
    refs,
  };
}

/** The message without its first line, trimmed; '' when none. */
export function commitBody(message: string): string {
  const nl = message.indexOf('\n');
  return nl < 0 ? '' : message.slice(nl + 1).trim();
}

/** Answers kept for the session, by sha: a range by `base..head`, the PRs by commit sha. Memory only (a PR can be opened later). */
export interface ContextCache {
  ranges: Map<string, { commits: CommitText[]; total: number }>;
  pulls: Map<string, PullRequest[]>;
}

export function contextCache(): ContextCache {
  return { ranges: new Map(), pulls: new Map() };
}

export interface ContextState {
  open: boolean;
  /** The list: null before it loaded. */
  commits: CommitText[] | null;
  total: number;
  /** Loading or failure line of the list; null: none. */
  status: string | null;
  /** The list failed: `status` holds why. */
  failed: boolean;
  /** The sha of the open commit; null: none. */
  current: string | null;
  /** PRs of the current commit; null while loading or after a failure. */
  pulls: PullRequest[] | null;
  /** Loading or failure line of the PRs. */
  pullStatus: string | null;
  /** The pull requests failed: `pullStatus` holds why. */
  pullFailed: boolean;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The state machine behind the commit context: nothing is asked before the first `toggle`; the
 * list of the range loads once, the pull requests of a commit when it opens. `onState` gets a
 * new state object after each change.
 */
export function commitContext(d: {
  source: CommitSource;
  repo: Repo;
  range: Range;
  cache: ContextCache;
  onState: (s: ContextState) => void;
}): { toggle(): Promise<void>; openCommit(sha: string): Promise<void> } {
  // The repository by its names and host, so that two hosts never share an answer.
  const at = JSON.stringify([d.repo.origin ?? null, d.repo.owner, d.repo.name]);
  const rangeKey = `${at}@${d.range.base}..${d.range.head}`;
  let s: ContextState = { open: false, commits: null, total: 0, status: null, failed: false, current: null, pulls: null, pullStatus: null, pullFailed: false };
  const set = (p: Partial<ContextState>) => {
    s = { ...s, ...p };
    d.onState(s);
  };
  let listing: Promise<void> | null = null;
  /** Pull request requests in flight, by the key of the cache. */
  const pending = new Map<string, Promise<PullRequest[]>>();

  async function load(): Promise<void> {
    const kept = d.cache.ranges.get(rangeKey);
    if (kept) return set({ commits: kept.commits, total: kept.total, status: null });
    // The commit list or the resolve of the overlay gave the commit with its message: no call.
    const known = d.range.commit;
    if (known?.message !== undefined && known.sha === d.range.head && (known.parents[0] ?? null) === d.range.base)
      return set({ commits: [{ ...known, message: known.message }], total: 1, status: null });
    set({ status: 'loading the commits …', failed: false });
    try {
      const listed = await d.source.between(d.repo, d.range.base, d.range.head);
      // A base that is the first parent of the head: the overlay is that one commit, also when
      // it is a merge, whose range lists the side branch too.
      const head = listed.commits.find((c) => c.sha === d.range.head);
      const r = head && (head.parents[0] ?? null) === d.range.base ? { commits: [head], total: 1 } : listed;
      d.cache.ranges.set(rangeKey, r);
      set({ commits: r.commits, total: r.total, status: null });
    } catch (e) {
      set({ status: `cannot list the commits: ${message(e)}`, failed: true });
    }
  }

  async function openCommit(sha: string): Promise<void> {
    if (s.current === sha) return set({ current: null, pulls: null, pullStatus: null, pullFailed: false });
    const key = `${at}@${sha}`;
    const kept = d.cache.pulls.get(key);
    if (kept) return set({ current: sha, pulls: kept, pullStatus: null, pullFailed: false });
    set({ current: sha, pulls: null, pullStatus: 'loading the pull requests …', pullFailed: false });
    try {
      // A request still pending for this sha is reused: each call counts against the limit.
      let asked = pending.get(key);
      if (!asked) {
        asked = d.source.pullsOf(d.repo, sha).finally(() => pending.delete(key));
        pending.set(key, asked);
      }
      const pulls = await asked;
      d.cache.pulls.set(key, pulls);
      // The user may have opened another commit meanwhile.
      if (s.current === sha) set({ pulls, pullStatus: null });
    } catch (e) {
      if (s.current === sha) set({ pullStatus: `cannot list the pull requests: ${message(e)}`, pullFailed: true });
    }
  }

  return {
    async toggle() {
      set({ open: !s.open });
      if (!s.open || s.commits !== null) return;
      listing ??= load().finally(() => (listing = null));
      await listing;
      const now = s as ContextState; // `s` changed while waiting
      if (now.commits?.length === 1 && now.current === null) await openCommit(now.commits[0].sha);
    },
    openCommit,
  };
}
