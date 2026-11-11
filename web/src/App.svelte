<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import CommitList from './components/CommitList.svelte';
  import CommitStrip from './components/CommitStrip.svelte';
  import DetailPanel from './components/DetailPanel.svelte';
  import Diagram from './components/Diagram.svelte';
  import EdgeBar from './components/EdgeBar.svelte';
  import FocusBar from './components/FocusBar.svelte';
  import OpenButtons from './components/OpenButtons.svelte';
  import Toolbar from './components/Toolbar.svelte';
  import type { Offset } from './lib/drag';
  import { DEFAULT_EDGE_OPTIONS, edgePipeline, painted, type Drawn, type EdgeOptions } from './lib/edgeOptions';
  import { toElkGraph } from './lib/elkGraph';
  import { commitTotals, headOf, openChanged, pathToOpen } from './lib/commit';
  import { changedFiles } from './lib/files';
  import { focusActive, focusOn, NO_FOCUS, type Focus } from './lib/focus';
  import {
    buildTree,
    collapse,
    deeper,
    levelExpanded,
    levelUnfolded,
    mustConfirm,
    splitByLevel,
    stepBack,
    stepForward,
    type Depth,
    type DepthStep,
    type Tree,
  } from './lib/expansion';
  import { depthKey } from './lib/keys';
  import { decodeHash, encodeHash, freshHash, type Source } from './lib/hash';
  import { fetchRepo, openCache, type Commit, type Forge, type Repo, type TextCache } from './lib/forge';
  import { cachedCommits, commitRows, fetchOverlay, overlayNames, resolveRange, type OverlaySpec, type Range } from './lib/forgeDiff';
  import { github, parseRepo } from './lib/github';
  import { diffInWorker, indexInWorker } from './lib/indexProject';
  import { inputError, summarize, type Summary } from './lib/project';
  import { collectDir, fromEntry, type Collect, type DirEntry } from './lib/walk';
  import { runLayout, type Layout } from './lib/layout';
  import type { Graph, Level, View } from './lib/types';
  import init, { Explorer, isInput } from './lib/wasm/cg_wasm.js';

  // An import that a use covers is a duplicate, so the view never counts it (ADR 13).
  const MERGE_IMPORTS = true;
  // A container with packages and other children shows its packages and one placeholder (ADR 19).
  const FOLD_MIXED = true;

  let error = $state<string | null>(null);
  let graph = $state.raw<Graph | null>(null);
  /** null: a file, a directory or a GitHub repository. */
  let src = $state<Source | null>('graph');
  /** The GitHub repository shown, `owner/repo@sha` (ADR 0020); null for any other source. */
  let gh = $state<string | null>(null);
  let ghCache: Promise<TextCache> | null = null;
  /**
   * The GitHub repository shown (ADR 0020, 0021): the plain graph of the head and, once built,
   * the overlay. Both stay in memory, so a switch between them fetches nothing.
   */
  type Kept = { text: string; summary: Summary };
  let ghRepo = $state.raw<Repo | null>(null);
  let ghHead = $state.raw<Kept | null>(null);
  let ghOverlay = $state.raw<(Kept & { range: Range }) | null>(null);
  let ghMode = $state<Source>('graph');
  /** The last commits reachable from the sha at which the list was asked for; kept while its commits are picked. */
  let commitList = $state.raw<{ anchor: string; list: Commit[] } | null>(null);
  let listStatus = $state<string | null>(null);
  let listOpen = $state(false);
  const COMMITS = 30;
  /** The base of the overlay shown: undefined for the plain graph, null for a root commit. */
  const shownBase = $derived(gh && ghMode === 'commit' && ghOverlay ? ghOverlay.range.base : undefined);
  const ghForge = (): Forge => github({ fetch: (url, i) => fetch(url, i) });
  /** /graph.json is missing: show the start screen. */
  let missing = $state(false);
  /** What an opened directory gave; null for any other source. */
  let project = $state.raw<Summary | null>(null);
  /** Progress of opening a directory, e.g. "reading 307 files". */
  let phase = $state<string | null>(null);
  let openSeq = 0;
  let hasCommit = $state(false);
  let explorer = $state.raw<Explorer | null>(null);
  let tree = $state.raw<Tree | null>(null);
  let expanded = $state.raw<Set<string>>(new Set());
  /** Containers that show all children in spite of the kind fold or the focus (ADR 0019, 0011). */
  let unfolded = $state.raw<Set<string>>(new Set());
  let tests = $state(false);
  let focus = $state.raw<Focus>(NO_FOCUS);
  // Deep state: each consumer tracks only the fields it reads, so O1 does not trigger a layout.
  let opts = $state<EdgeOptions>({ ...DEFAULT_EDGE_OPTIONS, kinds: { ...DEFAULT_EDGE_OPTIONS.kinds } });
  let selected = $state<string | null>(null);
  let hoverNode = $state<string | null>(null);
  let offsets = $state.raw<Map<string, Offset>>(new Map());
  /** A layout together with the edge list it was computed for. */
  let laid = $state.raw<{ layout: Layout; drawn: Drawn } | null>(null);
  let busy = $state(false);
  let fitNext = true;
  let seq = 0;
  let lastHash = '';
  let diagram: ReturnType<typeof Diagram> | undefined = $state();

  /**
   * Nodes that the plain cut shows with every container open: without test code it drops the
   * containers of test code only, which no level button and no "+" can make visible.
   */
  const shown = $derived.by(() => {
    if (!explorer || !tree || tests) return undefined;
    const all = JSON.parse(explorer.view([...tree.children.keys()], false, MERGE_IMPORTS)) as View;
    return new Set(all.nodes);
  });
  const hashState = $derived(tree ? splitByLevel(tree, expanded, shown, unfolded) : null);
  const extra = $derived(hashState ? hashState.open.length + hashState.unfold.length : 0);
  const activeLevel = $derived(hashState && extra === 0 ? hashState.level : null);
  const partialLevel = $derived(hashState && extra > 0 ? { level: hashState.level, extra } : null);

  // O4 is part of the WASM view.
  // Always the focus cut of ADR 0011 with the kind fold of ADR 0019; with an empty focus it
  // folds by kind only.
  function cut(ex: Explorer, d: Depth): View {
    const f = focus;
    const json = ex.focusView(
      [...d.expanded],
      tests,
      MERGE_IMPORTS,
      f.nodes,
      f.changes && graph?.diff != null,
      f.neighbors,
      [...d.unfolded],
      FOLD_MIXED,
    );
    return JSON.parse(json) as View;
  }
  const depth = $derived<Depth>({ expanded, unfolded });
  const view = $derived(explorer && tree ? cut(explorer, depth) : null);
  /** What "+" would give; equal to `depth` when nothing visible can open or unfold. */
  const deeperSet = $derived(view && tree ? deeper(tree, view, depth, shown) : depth);
  const canDeeper = $derived(deeperSet.expanded.size > expanded.size || deeperSet.unfolded.size > unfolded.size);
  /** "+" history, so that "−" right after it returns to the same set (see `stepBack`). */
  let steps: DepthStep[] = [];
  /** A "+" that waits for a second click, valid only for the set it was computed for. */
  let asked = $state.raw<{ to: Depth; boxes: number } | null>(null);
  const askBoxes = $derived(asked && asked.to === deeperSet ? asked.boxes : null);
  // O2 → O3.
  const drawn = $derived(view && tree ? edgePipeline(view.edges, tree, opts, focusActive(focus)) : null);
  const diff = $derived(graph?.diff ?? null);
  // Without a commit graph the option has no effect, even when the hash carries it.
  const changesOnly = $derived(diff !== null && opts.changesOnly);
  const totals = $derived(graph && tree && diff ? commitTotals(graph, tree, tests) : null);
  const files = $derived(graph && tree && diff?.files ? changedFiles(graph, tree, tests) : null);
  /** Per module box: number of changed files that the test-code switch shows. */
  const fileCounts = $derived(
    new Map((files?.groups ?? []).flatMap((g) => (g.module === null ? [] : [[g.module, g.files.length] as const]))),
  );
  const paintedCount = $derived(
    laid
      ? laid.layout.edges.filter((le) =>
          painted(laid!.drawn.edges[le.index], opts.onSelection, selected, hoverNode, changesOnly),
        ).length
      : 0,
  );

  async function load(text: string, hash = location.hash) {
    try {
      await init();
      const g = JSON.parse(text) as Graph;
      const ex = new Explorer(text);
      explorer?.free();
      explorer = ex;
      graph = g;
      tree = buildTree(g.nodes);
      offsets = new Map();
      error = null;
      applyHash(hash);
    } catch (e) {
      error = `cannot load graph: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  function applyHash(hash: string) {
    if (!tree) return;
    const s = decodeHash(hash);
    tests = s.tests;
    Object.assign(opts, s.edges);
    expanded = new Set([...levelExpanded(tree, s.level, shown), ...s.open]);
    unfolded = new Set([...levelUnfolded(tree, s.level, shown), ...(s.unfold ?? [])]);
    focus = s.focus ?? NO_FOCUS;
    fitNext = true;
  }

  async function fetchSource(s: Source) {
    try {
      const r = await fetch(`/${s}.json`);
      // The dev and preview servers answer a missing file with index.html.
      if (s === 'graph' && !tree && (r.status === 404 || !r.headers.get('content-type')?.includes('json'))) {
        missing = true;
        return;
      }
      if (!r.ok || !r.headers.get('content-type')?.includes('json')) throw new Error(`HTTP ${r.status}`);
      const text = await r.text();
      src = s;
      gh = null;
      forgetGh();
      selected = null;
      project = null;
      await load(text);
    } catch (e) {
      error = `cannot load /${s}.json: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** Replaces the graph by one opened from disk or GitHub: by default nothing open, no focus, no selection. */
  async function loadOpened(text: string, summary: Summary | null, hash = freshHash(location.hash), from: string | null = null) {
    if (from === null) forgetGh();
    selected = null;
    src = null;
    gh = from;
    project = summary;
    await load(text, hash);
    if (graph) missing = false;
  }

  /** Indexes the Java and Rust sources of a chosen directory in a worker (ADR 0015). Only inputs are read. */
  async function openDirectory(collect: Collect) {
    const my = ++openSeq;
    try {
      await init();
      const t0 = performance.now();
      phase = 'scanning';
      const chosen = await collect(isInput, (n) => {
        if (my === openSeq) phase = `scanning: ${n} directories`;
      });
      if (my !== openSeq) return;
      console.info(`scanned ${chosen.root}: ${chosen.inputs.length} inputs ${Math.round(performance.now() - t0)} ms`);
      const err = inputError(chosen);
      if (err) {
        error = err;
        phase = null;
        return;
      }
      phase = `reading ${chosen.inputs.length} files of ${chosen.root}`;
      const paths = chosen.inputs.map((i) => i.path);
      const texts = await Promise.all(chosen.inputs.map((i) => i.file.text()));
      if (my !== openSeq) return;
      phase = `indexing ${chosen.inputs.length} files of ${chosen.root}`;
      const r = await indexInWorker({ paths, texts, rootName: chosen.root });
      if (my !== openSeq) return;
      const g = JSON.parse(r.graphText) as Graph;
      console.info(`indexed ${chosen.root}: ${g.nodes.length} nodes ${g.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
      phase = null;
      await loadOpened(r.graphText, summarize(chosen.root, paths, g, r.report));
    } catch (e) {
      if (my !== openSeq) return;
      phase = null;
      error = `cannot index the directory: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  function forgetGh() {
    ghRepo = null;
    ghHead = null;
    ghOverlay = null;
    ghMode = 'graph';
    commitList = null;
    listOpen = false;
  }

  /** The text of the GitHub field: a clear message for anything that is not a repository (G1). */
  function openGithubInput(input: string) {
    const p = parseRepo(input);
    if (!p.ok) error = p.error;
    else if (p.overlay) openOverlay(p.repo, p.overlay, undefined, p.note);
    else openGithub(p.repo, p.note);
  }

  /**
   * Fetches the inputs of a public GitHub repository and indexes them as a directory (ADR 0020).
   * `hash` is the view to show afterwards; a failed open leaves the present graph as it is (G6).
   */
  async function openGithub(repo: Repo, note?: string, hash?: string) {
    const my = ++openSeq;
    const id = `${repo.owner}/${repo.name}`;
    try {
      await init();
      const t0 = performance.now();
      phase = `opening ${id}`;
      ghCache ??= openCache();
      const f = await fetchRepo(github({ fetch: (url, i) => fetch(url, i) }), repo, await ghCache, isInput, (p) => {
        if (my === openSeq) phase = p;
      });
      if (my !== openSeq) return;
      console.info(`fetched ${id}@${f.sha}: ${f.paths.length} inputs ${Math.round(performance.now() - t0)} ms`);
      phase = `indexing ${f.paths.length} files of ${id}`;
      const r = await indexInWorker({ paths: f.paths, texts: f.texts, rootName: f.root });
      if (my !== openSeq) return;
      const g = JSON.parse(r.graphText) as Graph;
      console.info(`indexed ${id}: ${g.nodes.length} nodes ${g.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
      phase = null;
      const short = f.sha.slice(0, 7);
      const label = repo.ref && repo.ref.toLowerCase() !== f.sha ? `${id}@${repo.ref} (${short})` : `${id}@${short}`;
      const summary = summarize(label, f.paths, g, r.report, `Fetched from GitHub at ${f.sha}; cached by blob sha in this browser. A reload opens the same commit.`);
      if (note) summary.text += ` (${note})`;
      if (commitList?.anchor !== f.sha) commitList = null;
      ghRepo = { owner: repo.owner, name: repo.name };
      ghHead = { text: r.graphText, summary };
      ghOverlay = null;
      ghMode = 'graph';
      listOpen = false;
      await loadOpened(r.graphText, summary, hash, `${id}@${f.sha}`);
    } catch (e) {
      if (my !== openSeq) return;
      phase = null;
      error = `cannot open ${id}: ${e instanceof Error ? e.message : String(e)}`;
      // Opened from the hash with nothing to show: offer the open buttons.
      if (!tree) missing = true;
    }
  }

  /**
   * Builds the commit overlay of a GitHub repository in this browser (ADR 0021): resolve the
   * range (none for a range of shas), two trees, the files that the cache lacks, `diffSources`
   * in the worker. A failed overlay leaves the present picture (C6). `keepList`: picked from the
   * commit list, which stays.
   */
  async function openOverlay(repo: Repo, how: OverlaySpec | Range, hash?: string, note?: string, keepList = false) {
    const my = ++openSeq;
    const id = `${repo.owner}/${repo.name}`;
    try {
      await init();
      const t0 = performance.now();
      phase = `opening ${id}`;
      const forge = ghForge();
      ghCache ??= openCache();
      const cache = await ghCache;
      let range: Range;
      if ('kind' in how) {
        phase = `resolving ${how.kind === 'pull' ? `pull request #${how.n}` : how.kind === 'commit' ? `commit ${repo.ref}` : `${how.base}${how.mergeBase ? '...' : '..'}${repo.ref}`} of ${id}`;
        range = await resolveRange(forge, repo, how);
      } else range = how;
      if (my !== openSeq) return;
      if (gh === `${id}@${range.head}` && ghOverlay && ghOverlay.range.base === range.base) {
        phase = null;
        await showGh('commit', hash);
        return;
      }
      const inputs = await fetchOverlay(forge, repo, range, cache, isInput, (p) => {
        if (my === openSeq) phase = p;
      });
      if (my !== openSeq) return;
      const names = overlayNames(range);
      console.info(`fetched ${id} ${names.base}..${names.head}: ${inputs.base.paths.length} + ${inputs.head.paths.length} inputs, ${inputs.changed.length} changed files ${Math.round(performance.now() - t0)} ms`);
      phase = `indexing ${names.base} → ${names.head} of ${id}`;
      const r = await diffInWorker({ rootName: inputs.root, base: inputs.base, head: inputs.head, baseName: names.base, headName: names.head, changed: inputs.changed });
      if (my !== openSeq) return;
      const overlay = JSON.parse(r.graphText) as Graph;
      const head = headOf(overlay);
      console.info(`overlay ${id} ${names.base}..${names.head}: ${overlay.nodes.length} nodes ${overlay.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
      phase = null;
      const origin = `Fetched from GitHub; cached by blob sha in this browser. The overlay of ${range.base ?? 'nothing'} to ${range.head} is built here; a reload shows the same overlay.`;
      const headSummary = summarize(`${id}@${names.head}`, inputs.head.paths, head, r.report, origin);
      if (note) headSummary.text += ` (${note})`;
      const overlaySummary = summarize(`${id} ${names.base} → ${names.head}`, inputs.head.paths, overlay, r.report, origin);
      overlaySummary.text += `, ${inputs.changed.length} changed files${note ? ` (${note})` : ''}`;
      if (!keepList) {
        commitList = null;
        listOpen = false;
      }
      ghRepo = { owner: repo.owner, name: repo.name };
      ghHead = { text: JSON.stringify(head), summary: headSummary };
      ghOverlay = { text: r.graphText, summary: overlaySummary, range };
      ghMode = 'commit';
      await loadOpened(r.graphText, overlaySummary, hash, `${id}@${range.head}`);
    } catch (e) {
      if (my !== openSeq) return;
      phase = null;
      error = `cannot build the overlay of ${id}: ${e instanceof Error ? e.message : String(e)}`;
      if (!tree) missing = true;
    }
  }

  /** Switches between the plain graph of the head and the overlay, both in memory: no fetch (C5). */
  async function showGh(mode: Source, hash = location.hash) {
    if (!gh) return;
    if (mode === 'graph') {
      if (!ghHead || ghMode === 'graph') return;
      ghMode = 'graph';
      await loadOpened(ghHead.text, ghHead.summary, hash, gh);
      return;
    }
    if (ghMode === 'commit') return;
    ghMode = 'commit';
    if (ghOverlay) await loadOpened(ghOverlay.text, ghOverlay.summary, hash, gh);
  }

  /** The commit list of C3: one call, none when cached; anchored at the sha shown when first asked. */
  async function loadCommits() {
    if (!ghRepo || !gh || commitList) return;
    const repo = ghRepo;
    const anchor = gh.slice(gh.lastIndexOf('@') + 1);
    listStatus = `listing the last ${COMMITS} commits`;
    try {
      ghCache ??= openCache();
      const list = await cachedCommits(ghForge(), repo, anchor, COMMITS, await ghCache);
      if (ghRepo !== repo) return;
      commitList = { anchor, list };
      listStatus = null;
    } catch (e) {
      listStatus = `cannot list the commits: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  function pickCommit(sha: string) {
    const c = commitList?.list.find((x) => x.sha === sha);
    if (!c || !ghRepo) return;
    openOverlay({ ...ghRepo, ref: c.sha }, { base: c.parents[0] ?? null, head: c.sha }, undefined, undefined, true);
  }

  /** A reload or a shared link: the same commit, with the view that the hash holds (G5); with `base`, the same overlay (C5). */
  function openFromHash(spec: string, hash: string, base: string | null | undefined) {
    const p = parseRepo(spec);
    if (!p.ok) {
      error = p.error;
      if (!tree) missing = true;
    } else if (base === undefined) {
      if (gh === spec && ghHead) showGh('graph', hash);
      else openGithub(p.repo, undefined, hash);
    } else {
      // The app writes full shas, which cost no call; a hand-written ref is resolved.
      const full = (x: string | null | undefined) => typeof x === 'string' && /^[0-9a-f]{40}$/i.test(x);
      const head = p.repo.ref;
      if (full(head) && (base === null || full(base)))
        openOverlay(p.repo, { base: base?.toLowerCase() ?? null, head: head!.toLowerCase() }, hash);
      else if (base === null) openOverlay(p.repo, { kind: 'commit' }, hash);
      else openOverlay(p.repo, { kind: 'range', base, mergeBase: false }, hash);
    }
  }

  // A directory dropped anywhere on the page opens like a chosen one. The counter follows
  // enter and leave over child elements, so the hint does not flicker.
  let dragDepth = $state(0);
  const carriesFiles = (e: DragEvent) => !!e.dataTransfer?.types.includes('Files');

  function onDrop(e: DragEvent) {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    // The entries must be taken during the event: the data transfer is empty afterwards.
    const entry = e.dataTransfer?.items[0]?.webkitGetAsEntry();
    if (entry?.isDirectory) openDirectory(collectDir(fromEntry(entry as unknown as DirEntry)));
    else error = 'drop a directory to index it';
  }

  /** Expands the containers on the path to `id` and selects it. */
  function reveal(id: string) {
    if (!tree) return;
    const path = pathToOpen(tree, id);
    if (path.some((p) => !expanded.has(p))) {
      expanded = new Set([...expanded, ...path]);
      fitNext = true;
    }
    selected = id;
  }

  function setLevel(l: Level) {
    if (!tree) return;
    expanded = levelExpanded(tree, l, shown);
    unfolded = levelUnfolded(tree, l, shown);
    fitNext = true;
  }

  function stepDeeper() {
    if (!explorer || !canDeeper) return;
    const next = deeperSet;
    // The count comes from the WASM cut of the next set, not from a copy of the cut.
    const boxes = askBoxes ?? cut(explorer, next).nodes.length;
    if (mustConfirm(boxes, askBoxes !== null)) {
      asked = { to: next, boxes };
      return;
    }
    asked = null;
    steps = stepForward(steps, depth, next);
    expanded = new Set(next.expanded);
    unfolded = new Set(next.unfolded);
    fitNext = true;
  }

  function stepShallower() {
    if (!tree || expanded.size === 0) return;
    const r = stepBack(tree, steps, depth, shown);
    steps = r.steps;
    expanded = new Set(r.depth.expanded);
    unfolded = new Set(r.depth.unfolded);
    fitNext = true;
  }

  function onKey(e: KeyboardEvent) {
    const k = depthKey(e);
    if (k === null) return;
    e.preventDefault();
    if (k === 'deeper') stepDeeper();
    else stepShallower();
  }

  function setFocus(f: Focus) {
    focus = f;
    fitNext = true;
  }

  function toggle(id: string) {
    if (!tree) return;
    if (expanded.has(id)) expanded = collapse(tree, expanded, id);
    else if (view?.expandable.includes(id)) expanded = new Set([...expanded, id]);
  }

  $effect(() => {
    const v = view;
    untrack(() => {
      if (v && selected && !v.nodes.includes(selected)) selected = null;
    });
  });

  // Layout from ELK in a worker. Stale results are dropped.
  $effect(() => {
    if (!view || !tree || !drawn) return;
    const d = drawn;
    const v = view;
    const my = ++seq;
    busy = true;
    const t0 = performance.now();
    runLayout(toElkGraph({ ...v, edges: d.edges }, tree, undefined, shown))
      .then(async (l) => {
        if (my !== seq) return;
        console.info(`layout ${v.nodes.length} nodes ${l.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
        laid = { layout: l, drawn: d };
        busy = false;
        if (fitNext) {
          fitNext = false;
          await tick();
          diagram?.fit();
        }
      })
      .catch((e) => {
        if (my !== seq) return;
        busy = false;
        error = `layout failed: ${e instanceof Error ? e.message : String(e)}`;
      });
  });

  // Hash follows the state.
  $effect(() => {
    if (!hashState) return;
    const h = encodeHash({
      src: src ?? undefined,
      gh: gh ?? undefined,
      base: shownBase,
      level: hashState.level,
      tests,
      open: hashState.open,
      unfold: hashState.unfold,
      edges: opts,
      focus: focusActive(focus) ? focus : undefined,
    });
    if (h !== location.hash) history.replaceState(null, '', h);
    lastHash = h;
  });

  onMount(() => {
    const onHash = () => {
      if (location.hash === lastHash) return;
      const d = decodeHash(location.hash);
      if (d.gh) {
        if (d.gh !== gh || d.base !== shownBase) openFromHash(d.gh, location.hash, d.base);
        else applyHash(location.hash);
        return;
      }
      const s = d.src ?? 'graph';
      if (s !== src) fetchSource(s);
      else applyHash(location.hash);
    };
    window.addEventListener('hashchange', onHash);
    // The dev and preview servers answer unknown paths with index.html, so check the type.
    fetch('/commit.json', { method: 'HEAD' })
      .then((r) => (hasCommit = r.ok && !!r.headers.get('content-type')?.includes('json')))
      .catch(() => (hasCommit = false));
    const start = decodeHash(location.hash);
    if (start.gh) openFromHash(start.gh, location.hash, start.base);
    else fetchSource(start.src ?? 'graph');
    return () => window.removeEventListener('hashchange', onHash);
  });
</script>

<svelte:window
  onkeydown={onKey}
  ondragenter={(e) => {
    if (carriesFiles(e)) dragDepth++;
  }}
  ondragleave={(e) => {
    if (carriesFiles(e)) dragDepth = Math.max(0, dragDepth - 1);
  }}
  ondragover={(e) => {
    if (carriesFiles(e)) e.preventDefault();
  }}
  ondrop={onDrop}
/>

{#if dragDepth > 0}
  <div
    class="pointer-events-none fixed inset-3 z-50 flex items-center justify-center border-2 border-dashed border-[var(--accent)] bg-[var(--bg)]/80 text-lg"
    data-drop-hint
  >
    drop a project directory to index it in this browser
  </div>
{/if}

<div class="flex h-full flex-col">
  <Toolbar
    level={activeLevel}
    partial={partialLevel}
    depth={{
      canDeeper,
      canShallower: expanded.size > 0,
      ask: askBoxes,
      ondeeper: stepDeeper,
      onshallower: stepShallower,
    }}
    {tests}
    {busy}
    moved={offsets.size > 0}
    src={gh ? ghMode : src}
    hasCommit={gh ? true : hasCommit}
    onsrc={(s) => {
      if (gh) {
        showGh(s);
        // Without an overlay the commit view is the list to pick one from.
        if (s === 'commit' && !ghOverlay) {
          listOpen = true;
          loadCommits();
        }
      } else if (s !== src) fetchSource(s);
    }}
    onlevel={setLevel}
    ontests={(v) => (tests = v)}
    onfit={() => diagram?.fit()}
    onreset={() => (offsets = new Map())}
    onfile={(f) => f.text().then((t) => loadOpened(t, null))}
    ondir={openDirectory}
    ongithub={openGithubInput}
  />
  {#if phase}
    <div class="border-b border-[var(--line)] px-3 py-1 text-[var(--muted)]" data-phase>{phase}…</div>
  {:else if project}
    <div class="border-b border-[var(--line)] px-3 py-1 text-[var(--muted)]" title={project.tooltip} data-project>
      {project.text}
    </div>
  {/if}
  <EdgeBar {opts} onchange={(patch) => Object.assign(opts, patch)} drawn={paintedCount} total={drawn?.total ?? 0} dropped={drawn?.dropped ?? 0} />
  {#if diff && totals}
    <CommitStrip
      {diff}
      {totals}
      changesOnly={opts.changesOnly}
      onchanges={(v) => (opts.changesOnly = v)}
      {files}
      onopen={() => {
        if (!tree) return;
        expanded = openChanged(tree, tests);
        unfolded = new Set();
        setFocus({ nodes: [], changes: true, neighbors: focus.neighbors });
      }}
      {listOpen}
      onlist={gh
        ? () => {
            listOpen = !listOpen;
            if (listOpen) loadCommits();
          }
        : undefined}
    />
  {/if}
  {#if gh && ghMode === 'commit' && listOpen}
    <CommitList rows={commitList ? commitRows(commitList.list, ghOverlay?.range ?? null) : null} status={listStatus} onpick={pickCommit} />
  {/if}
  {#if tree && focusActive(focus)}
    <FocusBar
      {tree}
      {focus}
      rest={view?.rest}
      unfolded={unfolded.size}
      onneighbors={(v) => setFocus({ ...focus, neighbors: v })}
      onclear={() => setFocus(NO_FOCUS)}
    />
  {/if}
  {#if error}
    <div class="border-b border-[var(--line)] px-3 py-1 text-[var(--accent)]">{error}</div>
  {/if}
  <div class="flex min-h-0 flex-1">
    {#if tree && view && laid}
      <Diagram
        bind:this={diagram}
        bind:hoverNode
        {tree}
        {view}
        layout={laid.layout}
        drawn={laid.drawn}
        onSelection={opts.onSelection}
        commit={diff !== null}
        {changesOnly}
        {offsets}
        {expanded}
        {selected}
        {fileCounts}
        onselect={(id) => (selected = id)}
        onunfold={(c) => (unfolded = new Set([...unfolded, c]))}
        {shown}
        focused={focusActive(focus)}
        ontoggle={toggle}
        onoffset={(id, o) => (offsets = new Map(offsets).set(id, o))}
      />
      <DetailPanel
        {tree}
        {view}
        {selected}
        {tests}
        commit={diff !== null}
        {files}
        onselect={(id) => (selected = id)}
        onreveal={reveal}
        onfocus={(id) => {
          if (!tree) return;
          const r = focusOn(tree, id);
          expanded = r.expanded;
          unfolded = new Set();
          setFocus(r.focus);
        }}
      />
    {:else if missing}
      <div class="flex flex-1 items-center justify-center">
        <div class="flex max-w-md flex-col gap-3" data-start>
          <p>No <code>graph.json</code> here. Open a project directory, or drop one anywhere on this page, to index it in this browser (Java and Rust). Or give a public GitHub repository. Or open a graph file made by <code>cg index</code>.</p>
          <p class="text-[var(--muted)]">A local directory is not stored: a reload returns to this screen. A GitHub repository stays in the link.</p>
          <div class="flex flex-wrap gap-2">
            <OpenButtons
              btn="px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]"
              onfile={(f) => f.text().then((t) => loadOpened(t, null))}
              ondir={openDirectory}
              ongithub={openGithubInput}
            />
          </div>
        </div>
      </div>
    {:else}
      <div class="flex-1"></div>
    {/if}
  </div>
</div>
