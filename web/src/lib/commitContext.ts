// The commit context (ADR 0030): the full message of the commits of an overlay, their pull
// requests and the issue references that the texts name, on demand. Pure but for the
// `CommitSource` that a caller passes in.
import type { CommitSource, CommitText, PullRequest, Repo } from './forge';
import type { Range } from './forgeDiff';

/** https://github.com/o/r, or {repo.origin}/o/r */
export function webBase(repo: Repo): string {
  return `${repo.origin ?? 'https://github.com'}/${repo.owner}/${repo.name}`;
}

export interface IssueRef {
  owner: string;
  name: string;
  n: number;
  /** '#12' in the same repository, 'o/r#12' else */
  label: string;
  url: string;
}

const ORIGIN_GITHUB = 'https://github.com';

/**
 * References `#12` and `owner/repo#12` in the text, and URLs of an issue or pull request of the
 * same host, in order of first appearance, without duplicates. A reference is a guess: it is
 * read from the text, not checked (ADR 0030).
 */
export function issueRefs(text: string, repo: Repo): IssueRef[] {
  const origin = repo.origin ?? ORIGIN_GITHUB;
  const pull = repo.origin === undefined ? 'pull' : 'pulls';
  const esc = origin.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const re = new RegExp(
    `${esc}/(?<uo>[\\w.-]+)/(?<ur>[\\w.-]+)/(?:issues|${pull})/(?<un>\\d+)(?!\\w)` +
      `|(?<![\\w/&.])(?:(?<ro>[\\w.-]+)/(?<rr>[\\w.-]+))?#(?<rn>\\d+)(?!\\w)`,
    'gi',
  );
  const out: IssueRef[] = [];
  const seen = new Set<string>();
  for (const m of text.matchAll(re)) {
    const g = m.groups!;
    const owner = g.uo ?? g.ro ?? repo.owner;
    const name = g.ur ?? g.rr ?? repo.name;
    const n = Number(g.un ?? g.rn);
    const key = `${owner}/${name}#${n}`.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    const same = owner.toLowerCase() === repo.owner.toLowerCase() && name.toLowerCase() === repo.name.toLowerCase();
    out.push({ owner, name, n, label: same ? `#${n}` : `${owner}/${name}#${n}`, url: `${origin}/${owner}/${name}/issues/${n}` });
  }
  return out;
}

export interface CommitView {
  sha: string;
  short: string;
  title: string;
  body: string;
  /** webBase/commit/sha */
  url: string;
  pulls: (PullRequest & { label: string })[];
  refs: IssueRef[];
}

/** The parts of a commit for display. refs: from the message and all PR bodies, minus those that name a listed PR of the same repository. */
export function commitView(repo: Repo, c: CommitText, pulls: PullRequest[]): CommitView {
  const listed = new Set(pulls.map((p) => p.number));
  const refs = issueRefs([c.message, ...pulls.map((p) => p.body)].join('\n'), repo).filter(
    (r) => !(r.label === `#${r.n}` && listed.has(r.n)),
  );
  return {
    sha: c.sha,
    short: c.sha.slice(0, 7),
    title: c.title,
    body: commitBody(c.message),
    url: `${webBase(repo)}/commit/${c.sha}`,
    // Built here: no URL of an answer is followed (ADR 0022, T-2).
    pulls: pulls.map((p) => ({ ...p, label: `#${p.number}`, url: `${webBase(repo)}/${repo.origin ? 'pulls' : 'pull'}/${p.number}` })),
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
  /** The sha of the open commit; null: none. */
  current: string | null;
  /** PRs of the current commit; null while loading or after a failure. */
  pulls: PullRequest[] | null;
  /** Loading or failure line of the PRs. */
  pullStatus: string | null;
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
  const rangeKey = `${webBase(d.repo)}@${d.range.base}..${d.range.head}`;
  let s: ContextState = { open: false, commits: null, total: 0, status: null, current: null, pulls: null, pullStatus: null };
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
    set({ status: 'loading the commits …' });
    try {
      const r = await d.source.between(d.repo, d.range.base, d.range.head);
      d.cache.ranges.set(rangeKey, r);
      set({ commits: r.commits, total: r.total, status: null });
    } catch (e) {
      set({ status: `cannot list the commits: ${message(e)}` });
    }
  }

  async function openCommit(sha: string): Promise<void> {
    if (s.current === sha) return set({ current: null, pulls: null, pullStatus: null });
    const key = `${webBase(d.repo)}@${sha}`;
    const kept = d.cache.pulls.get(key);
    if (kept) return set({ current: sha, pulls: kept, pullStatus: null });
    set({ current: sha, pulls: null, pullStatus: 'loading the pull requests …' });
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
      if (s.current === sha) set({ pullStatus: `cannot list the pull requests: ${message(e)}` });
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
