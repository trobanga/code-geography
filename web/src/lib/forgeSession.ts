// A repository of a forge in the viewer (ADR 0020, 0021): open the head, build an overlay,
// switch between both, the commit list, restore from the hash. No Svelte and no DOM: the app
// passes the forge, the cache, the index jobs and what to do with the results.
import { fetchRepo, type Branch, type Commit, type Forge, type Repo, type TextCache } from './forge';
import { fetchOverlay, listedCommits, overlayNames, rangeOfCommit, resolveRange, type OverlaySpec, type Parsed, type Range } from './forgeDiff';
import { headOf } from './commit';
import type { DiffRequest, IndexRequest } from './indexProject';
import { summarize, type CallReport, type Summary } from './project';
import type { Source } from './hash';
import type { Graph } from './types';

/** A graph kept in memory with its summary line, so that a switch fetches nothing. */
export interface Kept {
  text: string;
  summary: Summary;
}

export interface ForgeState {
  /** The repository shown, `owner/repo@sha` (ADR 0020); null for any other source. */
  at: string | null;
  repo: Repo | null;
  /** The plain graph of the head. */
  head: Kept | null;
  /** The overlay, once built. */
  overlay: (Kept & { range: Range }) | null;
  mode: Source;
  /** The last commits reachable from the sha at which the list was asked for; kept while its commits are picked. */
  commits: { anchor: string; list: Commit[]; /** The page that `loadMore` goes on from; null: the list holds the whole history. */ next: number | null } | null;
  listStatus: string | null;
  listOpen: boolean;
  /** The branches, as listed when the branch menu was opened last. */
  branches: Branch[] | null;
  branchStatus: string | null;
  branchOpen: boolean;
  /** The branch picked from the branch menu; null when the commit shown came another way. */
  branch: string | null;
}

export const NO_FORGE: ForgeState = {
  at: null,
  repo: null,
  head: null,
  overlay: null,
  mode: 'graph',
  commits: null,
  listStatus: null,
  listOpen: false,
  branches: null,
  branchStatus: null,
  branchOpen: false,
  branch: null,
};

/** The base of the overlay shown: undefined for the plain graph, null for a root commit. */
export function shownBase(s: ForgeState): string | null | undefined {
  return s.at && s.mode === 'commit' && s.overlay ? s.overlay.range.base : undefined;
}

/** The commit sha of `ForgeState.at`, which ends in `@<sha>`. */
export function shaOf(at: string): string {
  return at.slice(at.lastIndexOf('@') + 1);
}

/** The length of the commit list. */
const COMMITS = 30;

type Indexed = { graphText: string; report: { calls: CallReport } };

export interface SessionDeps {
  /** The forge of the repository's origin, with the token of that origin (ADR 0022); asked for each open. */
  forge: (repo: Repo) => Forge;
  /** The text of the input field or of the hash: a repository, a ref and maybe an overlay, or why it is not one. */
  parse: (input: string) => Parsed;
  cache: () => Promise<TextCache>;
  isInput: (path: string) => boolean;
  /** Resolves once `isInput` and the jobs work (the WASM module is loaded). */
  ready: () => Promise<unknown>;
  index: (req: IndexRequest) => Promise<Indexed>;
  diff: (req: DiffRequest) => Promise<Indexed>;
  /** Starts an open; the result tells whether it is still the latest, so that a stale reply changes nothing. */
  begin: () => () => boolean;
  /** The progress line; null: done. */
  phase: (p: string | null) => void;
  /** A failed open; the present graph stays. */
  fail: (message: string) => void;
  /** Shows a graph; `hash` is the view, undefined: a fresh one. */
  show: (text: string, summary: Summary, hash: string | undefined) => Promise<void>;
  /** The hash of the view shown now. */
  hash: () => string;
  onState: (s: ForgeState) => void;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The repository at a sha as the hash holds it: `owner/repo@sha` for GitHub, else `kind:host/owner/repo@sha` (ADR 0022). */
const atOf = (repo: Repo, sha: string) => `${repo.origin ? `${repo.kind}:${new URL(repo.origin).host}/` : ''}${repo.owner}/${repo.name}@${sha}`;
/** Only the repository, no ref. */
const bare = (repo: Repo): Repo => ({ owner: repo.owner, name: repo.name, ...(repo.origin ? { origin: repo.origin, kind: repo.kind } : {}) });

export function forgeSession(d: SessionDeps) {
  let s = NO_FORGE;
  const set = (patch: Partial<ForgeState>) => d.onState((s = { ...s, ...patch }));
  /** The branches listed for the repository shown, if `repo` is that one; else none. */
  const branchesFor = (repo: Repo) => (s.repo && atOf(s.repo, '') === atOf(repo, '') ? s.branches : null);

  /**
   * Fetches the inputs of a repository and indexes them as a directory (ADR 0020). `hash` is
   * the view to show afterwards; a failed open leaves the present graph as it is (G6).
   */
  async function open(repo: Repo, note?: string, hash?: string, branch?: string) {
    const current = d.begin();
    const id = `${repo.owner}/${repo.name}`;
    try {
      await d.ready();
      const t0 = performance.now();
      d.phase(`opening ${id}`);
      const forge = d.forge(repo);
      const f = await fetchRepo(forge, repo, await d.cache(), d.isInput, (p) => {
        if (current()) d.phase(p);
      });
      if (!current()) return;
      console.info(`fetched ${id}@${f.sha}: ${f.paths.length} inputs ${Math.round(performance.now() - t0)} ms`);
      d.phase(`indexing ${f.paths.length} files of ${id}`);
      const r = await d.index({ paths: f.paths, texts: f.texts, rootName: f.root });
      if (!current()) return;
      const g = JSON.parse(r.graphText) as Graph;
      console.info(`indexed ${id}: ${g.nodes.length} nodes ${g.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
      d.phase(null);
      const short = f.sha.slice(0, 7);
      const label = repo.ref && repo.ref.toLowerCase() !== f.sha ? `${id}@${repo.ref} (${short})` : `${id}@${short}`;
      const summary = summarize(label, f.paths, g, r.report, `Fetched from ${forge.name} at ${f.sha}; cached by blob sha in this browser. A reload opens the same commit.`);
      if (note) summary.text += ` (${note})`;
      set({
        at: atOf(repo, f.sha),
        repo: bare(repo),
        head: { text: r.graphText, summary },
        overlay: null,
        mode: 'graph',
        commits: s.commits?.anchor === f.sha ? s.commits : null,
        listOpen: false,
        branch: branch ?? null,
        branches: branchesFor(repo),
      });
      await d.show(r.graphText, summary, hash);
    } catch (e) {
      if (!current()) return;
      d.phase(null);
      d.fail(`cannot open ${id}: ${message(e)}`);
    }
  }

  /**
   * Builds the commit overlay in this browser (ADR 0021): resolve the range (none for a range
   * of shas), two trees, the files that the cache lacks, `diffSources` in a job. A failed
   * overlay leaves the present picture (C6). `keepList`: picked from the commit list, which stays.
   */
  async function overlay(repo: Repo, how: OverlaySpec | Range, hash?: string, note?: string, keepList = false) {
    const current = d.begin();
    const id = `${repo.owner}/${repo.name}`;
    try {
      await d.ready();
      const t0 = performance.now();
      d.phase(`opening ${id}`);
      const cache = await d.cache();
      const forge = d.forge(repo);
      let range: Range;
      if ('kind' in how) {
        d.phase(`resolving ${how.kind === 'pull' ? `pull request #${how.n}` : how.kind === 'commit' ? `commit ${repo.ref}` : `${how.base}${how.mergeBase ? '...' : '..'}${repo.ref}`} of ${id}`);
        range = await resolveRange(forge, repo, how);
      } else range = how;
      if (!current()) return;
      if (s.at === atOf(repo, range.head) && s.overlay && s.overlay.range.base === range.base) {
        d.phase(null);
        await show('commit', hash);
        return;
      }
      const inputs = await fetchOverlay(forge, repo, range, cache, d.isInput, (p) => {
        if (current()) d.phase(p);
      });
      if (!current()) return;
      const names = overlayNames(range);
      console.info(`fetched ${id} ${names.base}..${names.head}: ${inputs.base.paths.length} + ${inputs.head.paths.length} inputs, ${inputs.changed.length} changed files ${Math.round(performance.now() - t0)} ms`);
      d.phase(`indexing ${names.base} → ${names.head} of ${id}`);
      const r = await d.diff({ rootName: inputs.root, base: inputs.base, head: inputs.head, baseName: names.base, headName: names.head, changed: inputs.changed });
      if (!current()) return;
      const graph = JSON.parse(r.graphText) as Graph;
      const head = headOf(graph);
      console.info(`overlay ${id} ${names.base}..${names.head}: ${graph.nodes.length} nodes ${graph.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
      d.phase(null);
      const origin = `Fetched from ${forge.name}; cached by blob sha in this browser. The overlay of ${range.base ?? 'nothing'} to ${range.head} is built here; a reload shows the same overlay.`;
      const headSummary = summarize(`${id}@${names.head}`, inputs.head.paths, head, r.report, origin);
      if (note) headSummary.text += ` (${note})`;
      const summary = summarize(`${id} ${names.base} → ${names.head}`, inputs.head.paths, graph, r.report, origin);
      summary.text += `, ${inputs.changed.length} changed files${note ? ` (${note})` : ''}`;
      set({
        // A commit of the list stays on the branch it was listed from.
        ...(keepList ? {} : { commits: null, listOpen: false, branch: null }),
        branches: branchesFor(repo),
        at: atOf(repo, range.head),
        repo: bare(repo),
        head: { text: JSON.stringify(head), summary: headSummary },
        overlay: { text: r.graphText, summary, range },
        mode: 'commit',
      });
      await d.show(r.graphText, summary, hash);
    } catch (e) {
      if (!current()) return;
      d.phase(null);
      d.fail(`cannot build the overlay of ${id}: ${message(e)}`);
    }
  }

  /** A choice of the user that is no open: a pending open is stale from now on, and its progress line ends. */
  function cancel() {
    d.begin();
    d.phase(null);
  }

  /** Switches between the plain graph of the head and the overlay, both in memory: no fetch (C5). */
  async function show(mode: Source, hash = d.hash()) {
    if (!s.at) return;
    if (mode === 'graph') {
      if (!s.head || s.mode === 'graph') return;
      cancel();
      set({ mode: 'graph' });
      await d.show(s.head.text, s.head.summary, hash);
      return;
    }
    if (s.mode === 'commit') return;
    cancel();
    set({ mode: 'commit' });
    if (s.overlay) await d.show(s.overlay.text, s.overlay.summary, hash);
  }

  /**
   * A reload, a shared link or a changed hash: the same commit, with the view that the hash
   * holds (G5); with `base`, the same overlay (C5). False: the hash names what is shown, so
   * only its view applies.
   */
  function followHash(spec: string, hash: string, base: string | null | undefined): boolean {
    if (spec === s.at && base === shownBase(s)) return false;
    const p = d.parse(spec);
    if (!p.ok) d.fail(p.error);
    else if (base === undefined) {
      if (s.at === spec && s.head) show('graph', hash);
      else open(p.repo, undefined, hash);
    } else {
      // The app writes full shas, which cost no call; a hand-written ref is resolved.
      const full = (x: string | null | undefined) => typeof x === 'string' && /^[0-9a-f]{40}$/i.test(x);
      const head = p.repo.ref;
      if (full(head) && (base === null || full(base))) overlay(p.repo, { base: base?.toLowerCase() ?? null, head: head!.toLowerCase() }, hash);
      else if (base === null) overlay(p.repo, { kind: 'commit' }, hash);
      else overlay(p.repo, { kind: 'range', base, mergeBase: false }, hash);
    }
    return true;
  }

  /** The commit list of C3: one call, none when cached; anchored at the sha shown when first asked. */
  async function loadCommits() {
    if (!s.repo || !s.at || s.commits) return;
    const repo = s.repo;
    const anchor = shaOf(s.at);
    set({ listStatus: `listing the last ${COMMITS} commits` });
    try {
      const { list, next } = await listedCommits(d.forge(repo), repo, anchor, COMMITS, await d.cache());
      if (s.repo !== repo) return;
      set({ commits: { anchor, list, next }, listStatus: null });
    } catch (e) {
      set({ listStatus: `cannot list the commits: ${message(e)}` });
    }
  }

  /** The next commits of the history, after those of the list; the list stays while they load, and after a failure. */
  async function loadMore() {
    const had = s.commits;
    if (!s.repo || !had || had.next === null) return;
    const repo = s.repo;
    set({ listStatus: `listing ${COMMITS} more commits` });
    try {
      const { list, next } = await listedCommits(d.forge(repo), repo, had.anchor, COMMITS, await d.cache(), had.next);
      if (s.commits !== had) return;
      set({ commits: { anchor: had.anchor, list: [...had.list, ...list], next }, listStatus: null });
    } catch (e) {
      if (s.commits === had) set({ listStatus: `cannot list more commits: ${message(e)}` });
    }
  }

  /** Opens or closes the commit list. */
  function toggleList() {
    set(s.listOpen ? { listOpen: false } : { listOpen: true, branchOpen: false });
    if (s.listOpen) loadCommits();
  }

  /** Escape, or a click outside the list. */
  function closeList() {
    if (s.listOpen) set({ listOpen: false });
  }

  /** The branch menu: opens or closes it; each open lists the branches again, since they move. */
  function toggleBranches() {
    set(s.branchOpen ? { branchOpen: false } : { branchOpen: true, listOpen: false });
    if (s.branchOpen) loadBranches();
  }

  /** Escape, or a click outside the menu. */
  function closeBranches() {
    if (s.branchOpen) set({ branchOpen: false });
  }

  /** One call; the list shown before stays while it waits. */
  async function loadBranches() {
    if (!s.repo) return;
    const repo = s.repo;
    set({ branchStatus: 'listing the branches' });
    try {
      const branches = await d.forge(repo).branches(repo);
      if (s.repo === repo) set({ branches, branchStatus: null });
    } catch (e) {
      if (s.repo === repo) set({ branchStatus: `cannot list the branches: ${message(e)}` });
    }
  }

  /** A branch of the menu: the plain graph of its head with a fresh view; the menu closes at once. */
  async function pickBranch(name: string) {
    if (!s.repo || !s.branches?.some((b) => b.name === name)) return;
    set({ branchOpen: false });
    await open({ ...s.repo, ref: name }, undefined, undefined, name);
  }

  /** The source switch of the toolbar. Without an overlay the commit view is the list to pick one from. */
  function switchTo(mode: Source) {
    show(mode);
    if (mode === 'commit' && !s.overlay) {
      set({ listOpen: true });
      loadCommits();
    }
  }

  /** A commit of the list: its overlay against the first parent; the list closes at once and keeps its commits. */
  async function pick(sha: string) {
    const c = s.commits?.list.find((x) => x.sha === sha);
    if (!c || !s.repo) return;
    set({ listOpen: false });
    await overlay({ ...s.repo, ref: c.sha }, rangeOfCommit(c), undefined, undefined, true);
  }

  /**
   * A sha typed or pasted into the commit list, full or short: its overlay against the first
   * parent. A listed commit costs no call; any other is resolved by the forge and can be on
   * another branch, so the list and the branch do not stay.
   */
  async function pickSha(sha: string) {
    if (!s.repo) return;
    const c = s.commits?.list.find((x) => x.sha.startsWith(sha));
    if (c) return pick(c.sha);
    set({ listOpen: false });
    await overlay({ ...s.repo, ref: sha }, { kind: 'commit' });
  }

  /** The text of the input field: the message for anything that is not a repository (G1), else null. */
  function openInput(input: string): string | null {
    const p = d.parse(input);
    if (!p.ok) return p.error;
    if (p.overlay) overlay(p.repo, p.overlay, undefined, p.note);
    else open(p.repo, p.note);
    return null;
  }

  return {
    open,
    overlay,
    show,
    followHash,
    toggleList,
    closeList,
    toggleBranches,
    closeBranches,
    pickBranch,
    switchTo,
    loadMore,
    pick,
    pickSha,
    openInput,
    /** Another source is shown: nothing of the repository stays. */
    forget: () => {
      cancel();
      set(NO_FORGE);
    },
  };
}
