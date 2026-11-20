// A local repository with history in the viewer (ADR 0040): the commit list, the overlay of a
// commit or a range, the working tree as one entry of the list. No Svelte and no DOM: the app
// passes the worker handle and what to do with the results (the pattern of forgeSession.ts).
import type { Commit } from './forge';
import { LIST_PAGES, overlayNames, rangeOfCommit, withoutUpdates, type Range } from './forgeDiff';
import type { CommitOverlay } from './indexProject';
import { summarize, type Summary } from './project';
import type { SourceTexts } from './source';
import type { Graph } from './types';

export interface LocalState {
  /** A local repository with history is shown. */
  active: boolean;
  /** The working tree against `HEAD`, or the overlay of a range. */
  shown: 'worktree' | Range;
  /** The first-parent history from `HEAD`; `next`: the sha that the next page starts at, null: the history ended. */
  commits: { list: Commit[]; next: string | null } | null;
  listStatus: string | null;
  listOpen: boolean;
}

export const NO_LOCAL: LocalState = { active: false, shown: 'worktree', commits: null, listStatus: null, listOpen: false };

/** The part of `LocalRepoWorker` that the session uses. */
export interface LocalRepo {
  log(from: string | null, n: number): Promise<Commit[]>;
  overlay(base: string | null, head: string, rootName: string): Promise<CommitOverlay>;
  close(): void;
}

/** The working tree against `HEAD`, as built at the open; kept in memory so that choosing it again asks nobody. */
export interface KeptWorktree {
  text: string;
  summary: Summary;
  texts: SourceTexts;
}

export interface LocalDeps {
  /** Starts an overlay; the result tells whether it is still the latest, so that a stale reply changes nothing. */
  begin: () => () => boolean;
  /** The progress line; null: done. */
  phase: (p: string | null) => void;
  /** A failed overlay or listing; the picture stays. */
  fail: (message: string) => void;
  /** Shows a graph with the texts for its source view; the view is a fresh one. */
  show: (text: string, summary: Summary, texts: SourceTexts) => Promise<void>;
  onState: (s: LocalState) => void;
}

/** The length of a page of the commit list. */
const COMMITS = 30;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function localSession(d: LocalDeps) {
  let s = NO_LOCAL;
  let repo: LocalRepo | null = null;
  let rootName = '';
  let worktree: KeptWorktree | null = null;
  const set = (patch: Partial<LocalState>) => d.onState((s = { ...s, ...patch }));

  /** The page that `list` ends with, and where the next starts: null after the root commit or a short page. */
  const nextOf = (page: Commit[]) => (page.length < COMMITS ? null : (page[page.length - 1].parents[0] ?? null));

  /**
   * Pages of the history from `from` (null: `HEAD`) until `COMMITS` rows stay after the commits of
   * update bots go (`withoutUpdates`, as `listedCommits` does for a forge), or the history ends, or
   * after `LIST_PAGES` pages.
   */
  async function listed(mine: LocalRepo, from: string | null): Promise<{ list: Commit[]; next: string | null }> {
    const list: Commit[] = [];
    let next: string | null = from;
    for (let page = 0; page < LIST_PAGES; page++) {
      const more = await mine.log(next, COMMITS);
      list.push(...more);
      next = nextOf(more);
      if (next === null || withoutUpdates(list).length >= COMMITS) break;
    }
    return { list, next };
  }

  /** The first page, when the list opens. */
  async function loadCommits() {
    const mine = repo;
    if (!mine || s.commits) return;
    set({ listStatus: `listing the last ${COMMITS} commits` });
    try {
      const { list, next } = await listed(mine, null);
      if (repo !== mine) return;
      set({ commits: { list, next }, listStatus: null });
    } catch (e) {
      if (repo === mine) set({ listStatus: `cannot list the commits: ${message(e)}` });
    }
  }

  /** Opens or closes the commit list. */
  function toggleList() {
    if (!s.active) return;
    set({ listOpen: !s.listOpen });
    if (s.listOpen) loadCommits();
  }

  /** Escape, or a click outside the list. */
  function closeList() {
    if (s.listOpen) set({ listOpen: false });
  }

  /** The next page after those of the list; the list stays while it loads, and after a failure. */
  async function loadMore() {
    const mine = repo;
    const had = s.commits;
    if (!mine || !had || had.next === null) return;
    set({ listStatus: `listing ${COMMITS} more commits` });
    try {
      const { list, next } = await listed(mine, had.next);
      if (s.commits !== had) return;
      set({ commits: { list: [...had.list, ...list], next }, listStatus: null });
    } catch (e) {
      if (s.commits === had) set({ listStatus: `cannot list more commits: ${message(e)}` });
    }
  }

  /** The overlay of this range is the one shown. */
  const isShown = (range: Range) => s.shown !== 'worktree' && s.shown.head === range.head && s.shown.base === range.base;

  /**
   * Builds the overlay of a range in the worker and shows it. A failure leaves the present
   * picture; a stale reply (`current` false) changes nothing.
   */
  async function overlay(range: Range, current: () => boolean) {
    const mine = repo;
    if (!mine) return;
    const names = overlayNames(range);
    const t0 = performance.now();
    try {
      d.phase(`indexing ${names.base} → ${names.head} of ${rootName}`);
      const r = await mine.overlay(range.base, range.head, rootName);
      if (!current() || repo !== mine) return;
      const graph = JSON.parse(r.graphText) as Graph;
      console.info(`overlay ${rootName} ${names.base}..${names.head}: ${graph.nodes.length} nodes ${graph.edges.length} edges, ${r.indexes} indexes ${Math.round(performance.now() - t0)} ms`);
      d.phase(null);
      const origin = `Read from .git of the chosen directory. The overlay of ${range.base ?? 'nothing'} to ${range.head} is built in this browser; a reload returns to the start screen.`;
      const summary = summarize(`${rootName} ${names.base} → ${names.head}`, Object.keys(r.head), graph, r.report, origin);
      summary.text += `, ${graph.diff?.files?.length ?? 0} changed files`;
      const texts: SourceTexts = { head: new Map(Object.entries(r.head)), base: new Map(Object.entries(r.base)), names };
      set({ shown: range });
      await d.show(r.graphText, summary, texts);
    } catch (e) {
      if (!current()) return;
      d.phase(null);
      d.fail(`cannot build the overlay of ${rootName}: ${message(e)}`);
    }
  }

  /** A commit of the list: its overlay against the first parent; the list closes at once and keeps its commits. */
  async function pick(sha: string) {
    const c = s.commits?.list.find((x) => x.sha === sha);
    if (!c || !repo) return;
    set({ listOpen: false });
    const range = rangeOfCommit(c);
    const current = d.begin();
    // The commit shown already: a pending overlay of another one is stale now.
    if (isShown(range)) return d.phase(null);
    await overlay(range, current);
  }

  /** The commit that the repository names `name` by, with its overlay against its first parent. */
  async function pickSha(sha: string) {
    const c = s.commits?.list.find((x) => x.sha.startsWith(sha));
    if (c) return pick(c.sha);
    const mine = repo;
    if (!mine) return;
    set({ listOpen: false });
    const current = d.begin();
    try {
      d.phase(`resolving ${sha} of ${rootName}`);
      const found = (await mine.log(sha, 1))[0];
      if (!found) throw new Error(`no commit ${sha}`);
      if (!current() || repo !== mine) return;
      const range = rangeOfCommit(found);
      if (isShown(range)) d.phase(null);
      else await overlay(range, current);
    } catch (e) {
      if (!current()) return;
      d.phase(null);
      d.fail(`cannot open ${sha}: ${message(e)}`);
    }
  }

  /** `A..B`: the overlay of the tree of B against the tree of A; both ends are resolved to full shas first. */
  async function pickRange(base: string, head: string) {
    const mine = repo;
    if (!mine) return;
    set({ listOpen: false });
    const current = d.begin();
    try {
      d.phase(`resolving ${base}..${head} of ${rootName}`);
      const full = async (name: string) => {
        const found = (await mine.log(name, 1))[0];
        if (!found) throw new Error(`no commit ${name}`);
        return found.sha;
      };
      const range: Range = { base: await full(base), head: await full(head) };
      if (!current() || repo !== mine) return;
      if (isShown(range)) d.phase(null);
      else await overlay(range, current);
    } catch (e) {
      if (!current()) return;
      d.phase(null);
      d.fail(`cannot open the range: ${message(e)}`);
    }
  }

  /** The uncommitted changes: the overlay kept from the open, nothing is asked of the worker. A pending overlay is stale from now on. */
  async function pickWorktree() {
    set({ listOpen: false });
    if (!worktree) return;
    d.begin();
    d.phase(null);
    if (s.shown === 'worktree') return;
    set({ shown: 'worktree' });
    await d.show(worktree.text, worktree.summary, worktree.texts);
  }

  /** Another source is shown: the worker ends, nothing of the repository stays. A pending overlay changes nothing. */
  function forget() {
    if (!s.active) return;
    repo?.close();
    repo = null;
    worktree = null;
    d.phase(null);
    set(NO_LOCAL);
  }

  return {
    /** A repository was opened: its working tree is shown. A session before it ends. */
    start(handle: LocalRepo, root: string, kept: KeptWorktree) {
      if (repo && repo !== handle) repo.close();
      repo = handle;
      rootName = root;
      worktree = kept;
      s = NO_LOCAL;
      set({ active: true });
    },
    toggleList,
    closeList,
    loadMore,
    pick,
    pickWorktree,
    pickSha,
    pickRange,
    forget,
  };
}
