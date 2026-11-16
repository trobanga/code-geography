<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import CommitList from './components/CommitList.svelte';
  import BranchList from './components/BranchList.svelte';
  import CommitContext from './components/CommitContext.svelte';
  import CommitStrip from './components/CommitStrip.svelte';
  import { contextCache } from './lib/commitContext';
  import DetailPanel from './components/DetailPanel.svelte';
  import Diagram from './components/Diagram.svelte';
  import EdgeBar from './components/EdgeBar.svelte';
  import FocusBar from './components/FocusBar.svelte';
  import OpenButtons from './components/OpenButtons.svelte';
  import SourcePanel from './components/SourcePanel.svelte';
  import Survey from './components/Survey.svelte';
  import Toolbar from './components/Toolbar.svelte';
  import type { Offset } from './lib/drag';
  import { DEFAULT_EDGE_OPTIONS, edgePipeline, painted, type Drawn, type EdgeOptions } from './lib/edgeOptions';
  import { toElkGraph } from './lib/elkGraph';
  import { activeBoxes, commitTotals, openChangedStep, pathToOpen } from './lib/commit';
  import { changedFiles } from './lib/files';
  import { openOrClose, pick } from './lib/selection';
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
  import { cover } from './lib/cover';
  import { decodeHash, encodeHash, freshHash, type Source } from './lib/hash';
  import { openCache, type Repo, type TextCache } from './lib/forge';
  import { commitRows } from './lib/forgeDiff';
  import { forgeSession, NO_FORGE, shaOf, shownBase, type ForgeState } from './lib/forgeSession';
  import { forgeOfRepo, parseSource } from './lib/forges';
  import { tokenStore, type TokenField } from './lib/tokenStore';
  import { diffInWorker, indexInWorker } from './lib/indexProject';
  import { inputError, summarize, type Summary } from './lib/project';
  import { collectDir, fromEntry, type Collect, type DirEntry } from './lib/walk';
  import { runLayout, type Layout } from './lib/layout';
  import { sourceReader } from './lib/source';
  import type { Graph, GraphEdge, Level, View } from './lib/types';
  import init, { Explorer, isInput } from './lib/wasm/cg_wasm.js';

  // An import that a use covers is a duplicate, so the view never counts it (ADR 13).
  const MERGE_IMPORTS = true;
  // A container with packages and other children shows its packages and one placeholder (ADR 19).
  const FOLD_MIXED = true;

  let error = $state<string | null>(null);
  let graph = $state.raw<Graph | null>(null);
  /** null: a file, a directory or a GitHub repository. */
  let src = $state<Source | null>('graph');
  /** The repository of a forge shown (ADR 0020, 0021); `at` is null for any other source. */
  let fs = $state.raw<ForgeState>(NO_FORGE);
  let forgeCache: Promise<TextCache> | null = null;
  // Storage getters, since reading `sessionStorage` itself throws where site data is blocked.
  const tokens = tokenStore(
    () => globalThis.sessionStorage,
    () => globalThis.localStorage,
  );
  const net = { fetch: (url: string, i?: Parameters<typeof fetch>[1]) => fetch(url, i) };
  // A new forge per call, with the token of its origin as stored now (ADR 0022).
  const forgeOf = (repo: Repo) => forgeOfRepo(repo, net, repo.origin === undefined ? undefined : tokens.get(repo.origin));
  const contexts = contextCache();
  const forge = forgeSession({
    forge: forgeOf,
    parse: parseSource,
    cache: () => (forgeCache ??= openCache()),
    isInput,
    ready: init,
    index: indexInWorker,
    diff: diffInWorker,
    begin,
    phase: (p) => (phase = p),
    fail: (m) => {
      error = m;
      // Opened from the hash with nothing to show: offer the open buttons.
      if (!tree) missing = true;
    },
    show: (text, summary, hash) => loadOpened(text, summary, hash, true),
    hash: () => location.hash,
    onState: (s) => (fs = s),
  });
  /** The origin of a non-GitHub repository that the hash asks for: its token field shows also when the open failed. */
  let hashOrigin = $state<string | null>(null);
  let tokenRev = $state(0);
  const auth: TokenField = $derived({
    origin: fs.repo?.origin ?? hashOrigin,
    rev: tokenRev,
    kept: (o) => tokens.kept(o),
    save: (o, t, keep) => {
      tokens.set(o, t, keep);
      tokenRev++;
    },
    forget: (o) => {
      tokens.forget(o);
      tokenRev++;
      (forgeCache ??= openCache()).then((c) => c.clear()).catch(() => {});
    },
    retry: () => followForgeHash(location.hash, true),
  });
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
  /** The selected boxes, in the order of selection; Shift+click adds or removes one. */
  let selected = $state.raw<string[]>([]);
  let hoverNode = $state<string | null>(null);
  let offsets = $state.raw<Map<string, Offset>>(new Map());
  /** A layout together with the edge list it was computed for. */
  let laid = $state.raw<{ layout: Layout; drawn: Drawn } | null>(null);
  let busy = $state(false);
  /** A step that replaces the whole picture is in progress: the diagram is covered from before its cut to its layout. */
  let stepping = $state(false);
  const stepCover = cover({
    // The frame after the next one starts: the cover is on the screen then.
    paint: () => new Promise<void>((r) => requestAnimationFrame(() => setTimeout(r))),
    show: (on) => (stepping = on),
  });
  /** Runs a step behind the cover; `run` tells whether it changed the depth. */
  function covered(run: () => boolean) {
    stepCover.step(run).catch((e) => (error = `step failed: ${e instanceof Error ? e.message : String(e)}`));
  }
  let fitNext = true;
  /** A container that the user opened or closed: its box is brought into view after the next layout. */
  let inViewNext: string | null = null;
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
  /** What open / close does to the selected boxes now. */
  const selectionStep = $derived.by(() => {
    const next = tree && view ? openOrClose(tree, expanded, view.expandable, selected) : null;
    return next ? { expanded: next, step: next.size > expanded.size ? ('open' as const) : ('close' as const) } : null;
  });
  const view = $derived(explorer && tree ? cut(explorer, depth) : null);
  /** What "+" would give; equal to `depth` when nothing visible can open or unfold. */
  const deeperSet = $derived(view && tree ? deeper(tree, view, depth, shown) : depth);
  const canDeeper = $derived(deeperSet.expanded.size > expanded.size || deeperSet.unfolded.size > unfolded.size);
  /** "+" history, so that "−" right after it returns to the same set (see `stepBack`). */
  let steps: DepthStep[] = [];
  /** "+" or a level button that waits for a second click, valid only for the view it was asked in. */
  let asked = $state.raw<{ step: Level | 'deeper'; from: View; boxes: number } | null>(null);
  const pending = $derived(asked && asked.from === view ? asked : null);
  const askBoxes = $derived(pending?.step === 'deeper' ? pending.boxes : null);
  const askLevel = $derived(pending && pending.step !== 'deeper' ? { level: pending.step, boxes: pending.boxes } : null);
  const diff = $derived(graph?.diff ?? null);
  // Without a commit graph the option has no effect, even when the hash carries it.
  const changesOnly = $derived(diff !== null && opts.changesOnly);
  /** Changes together: the layout gets the changed lines only and puts the active boxes together. */
  const together = $derived(changesOnly && opts.changesTogether);
  // O2 → O3.
  const drawn = $derived(view && tree ? edgePipeline(view.edges, tree, opts, focusActive(focus), together) : null);
  const totals = $derived(graph && tree && diff ? commitTotals(graph, tree, tests) : null);
  const files = $derived(graph && tree && diff?.files ? changedFiles(graph, tree, tests) : null);
  /** Per module box: number of changed files that the test-code switch shows. */
  const fileCounts = $derived(
    new Map((files?.groups ?? []).flatMap((g) => (g.module === null ? [] : [[g.module, g.files.length] as const]))),
  );
  const active = $derived(together && view && tree && drawn ? activeBoxes(tree, view.changes, fileCounts, drawn.edges) : undefined);
  const paintedCount = $derived(
    laid
      ? laid.layout.edges.filter((le) =>
          painted(laid!.drawn.edges[le.index], opts.onSelection, selected, hoverNode, changesOnly),
        ).length
      : 0,
  );

  // The source view (ADR 0031). Three values, so that another change of `fs` makes no new reader.
  const sourceAt = $derived(fs.at);
  const sourceRepo = $derived(fs.repo);
  const sourceBase = $derived(shownBase(fs));
  /** Reads the lines of a node of the repository shown; null: the graph has no texts. */
  const readSource = $derived(
    sourceAt && sourceRepo
      ? sourceReader({
          forge: () => forgeOf(sourceRepo),
          repo: sourceRepo,
          cache: () => (forgeCache ??= openCache()),
          head: shaOf(sourceAt),
          base: sourceBase,
        })
      : null,
  );
  /** The edges behind the line from one box to another, from the graph in WASM. */
  const between = (from: string, to: string) =>
    explorer ? (JSON.parse(explorer.edgesBetween(from, to, tests, MERGE_IMPORTS)) as GraphEdge[]) : [];

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
      // The layout of the graph before this one does not fit the new one.
      laid = null;
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
      forge.forget();
      selected = [];
      project = null;
      await load(text);
    } catch (e) {
      error = `cannot load /${s}.json: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** Replaces the graph by one opened from disk or a forge (`keep`): by default nothing open, no focus, no selection. */
  async function loadOpened(text: string, summary: Summary | null, hash = freshHash(location.hash), keep = false) {
    if (!keep) forge.forget();
    selected = [];
    src = null;
    project = summary;
    await load(text, hash);
    if (graph) missing = false;
  }

  /** Indexes the Java, Rust and Clojure sources of a chosen directory in a worker (ADR 0015). Only inputs are read. */
  /** Starts an open of a directory or a repository; the result tells whether it is still the latest. */
  function begin() {
    const my = ++openSeq;
    return () => my === openSeq;
  }

  async function openDirectory(collect: Collect) {
    const current = begin();
    try {
      await init();
      const t0 = performance.now();
      phase = 'scanning';
      const chosen = await collect(isInput, (n) => {
        if (current()) phase = `scanning: ${n} directories`;
      });
      if (!current()) return;
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
      if (!current()) return;
      phase = `indexing ${chosen.inputs.length} files of ${chosen.root}`;
      const r = await indexInWorker({ paths, texts, rootName: chosen.root });
      if (!current()) return;
      const g = JSON.parse(r.graphText) as Graph;
      console.info(`indexed ${chosen.root}: ${g.nodes.length} nodes ${g.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
      phase = null;
      await loadOpened(r.graphText, summarize(chosen.root, paths, g, r.report));
    } catch (e) {
      if (!current()) return;
      phase = null;
      error = `cannot index the directory: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /**
   * A hash with `gh=` or `forge=`: the same commit (ADR 0020, 0022). False: it names what is shown.
   * `again`: after a token was given, also when the hash names what is shown.
   */
  function followForgeHash(hash: string, again = false): boolean {
    const d = decodeHash(hash);
    const spec = d.gh ?? d.forge;
    if (!spec) return false;
    const p = d.forge ? parseSource(d.forge) : null;
    hashOrigin = p?.ok ? (p.repo.origin ?? null) : null;
    if (again) error = null;
    return forge.followHash(spec, hash, d.base, d.branch);
  }

  /** The text of the repository field: a clear message for anything that is not a repository (G1). */
  function openForgeInput(input: string) {
    const e = forge.openInput(input);
    if (e) error = e;
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
    selected = [id];
  }

  /**
   * Whether the step to `next` may go now: above the box limit only at its second click.
   * The count comes from the WASM cut of the next set, not from a copy of the cut.
   */
  function confirmed(step: Level | 'deeper', next: Depth): boolean {
    if (!explorer || !view) return false;
    const again = pending?.step === step;
    const boxes = again ? pending!.boxes : cut(explorer, next).nodes.length;
    if (mustConfirm(boxes, again)) {
      asked = { step, from: view, boxes };
      return false;
    }
    asked = null;
    return true;
  }

  // The three steps below run behind the cover: the cuts and the layout input block the main
  // thread (0.5 s for the step to types on 200 000 lines), so the cover is painted first.
  function setLevel(l: Level) {
    covered(() => {
      if (!tree) return false;
      const next = { expanded: levelExpanded(tree, l, shown), unfolded: levelUnfolded(tree, l, shown) };
      if (!confirmed(l, next)) return false;
      expanded = next.expanded;
      unfolded = next.unfolded;
      fitNext = true;
      return true;
    });
  }

  function stepDeeper() {
    if (!canDeeper) return;
    covered(() => {
      if (!canDeeper) return false;
      const next = deeperSet;
      if (!confirmed('deeper', next)) return false;
      steps = stepForward(steps, depth, next);
      expanded = new Set(next.expanded);
      unfolded = new Set(next.unfolded);
      fitNext = true;
      return true;
    });
  }

  function stepShallower() {
    if (!tree || expanded.size === 0) return;
    covered(() => {
      if (!tree || expanded.size === 0) return false;
      const r = stepBack(tree, steps, depth, shown);
      steps = r.steps;
      expanded = new Set(r.depth.expanded);
      unfolded = new Set(r.depth.unfolded);
      fitNext = true;
      return true;
    });
  }

  function onKey(e: KeyboardEvent) {
    // Escape closes the branch menu or the commit list first, also from an input field.
    if (e.key === 'Escape' && fs.branchOpen) {
      e.preventDefault();
      forge.closeBranches();
      return;
    }
    if (e.key === 'Escape' && fs.listOpen) {
      e.preventDefault();
      forge.closeList();
      return;
    }
    const k = depthKey(e);
    if (k === null) return;
    e.preventDefault();
    if (k === 'deeper') stepDeeper();
    else if (k === 'shallower') stepShallower();
    else asked = null;
  }

  function setFocus(f: Focus) {
    focus = f;
    fitNext = true;
  }

  function toggle(id: string) {
    if (!tree) return;
    if (expanded.has(id)) expanded = collapse(tree, expanded, id);
    else if (view?.expandable.includes(id)) expanded = new Set([...expanded, id]);
    else return;
    inViewNext = id;
  }

  /** Open / close for two or more selected boxes: one step, one layout. */
  function stepSelection() {
    if (!selectionStep) return;
    expanded = selectionStep.expanded;
    fitNext = true;
  }

  function unfold(id: string) {
    unfolded = new Set([...unfolded, id]);
    inViewNext = id;
  }

  $effect(() => {
    const v = view;
    untrack(() => {
      if (v && selected.some((id) => !v.nodes.includes(id))) selected = selected.filter((id) => v.nodes.includes(id));
    });
  });

  // Layout from ELK in a worker. Stale results are dropped.
  $effect(() => {
    if (!view || !tree || !drawn) return;
    const d = drawn;
    const v = view;
    const a = active;
    const my = ++seq;
    busy = true;
    const t0 = performance.now();
    runLayout(toElkGraph({ ...v, edges: d.edges }, tree, undefined, shown, a))
      .then(async (l) => {
        if (my !== seq) return;
        console.info(`layout ${v.nodes.length} nodes ${l.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
        laid = { layout: l, drawn: d };
        busy = false;
        stepCover.landed();
        const show = inViewNext;
        inViewNext = null;
        if (fitNext) {
          fitNext = false;
          await tick();
          diagram?.fit();
        } else if (show !== null) {
          await tick();
          diagram?.reveal(show);
        }
      })
      .catch((e) => {
        if (my !== seq) return;
        busy = false;
        stepCover.landed();
        error = `layout failed: ${e instanceof Error ? e.message : String(e)}`;
      });
  });

  // Hash follows the state.
  $effect(() => {
    if (!hashState) return;
    const h = encodeHash({
      src: src ?? undefined,
      gh: fs.repo && !fs.repo.origin ? (fs.at ?? undefined) : undefined,
      forge: fs.repo?.origin ? (fs.at ?? undefined) : undefined,
      base: shownBase(fs),
      branch: fs.branch ?? undefined,
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
      if (d.gh || d.forge) {
        if (!followForgeHash(location.hash)) applyHash(location.hash);
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
    if (start.gh || start.forge) followForgeHash(location.hash);
    else fetchSource(start.src ?? 'graph');
    return () => window.removeEventListener('hashchange', onHash);
  });
</script>

<svelte:window
  onkeydown={onKey}
  onpointerdown={(e) => {
    // A click outside the commit list closes it. Not on its control: the click that follows toggles it.
    if (fs.listOpen && !(e.target as Element | null)?.closest?.('[data-commits], [data-list-toggle]')) forge.closeList();
    if (fs.branchOpen && !(e.target as Element | null)?.closest?.('[data-branches], [data-branch-toggle]')) forge.closeBranches();
  }}
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
    ask={askLevel}
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
    src={fs.at ? fs.mode : src}
    hasCommit={fs.at ? true : hasCommit}
    onsrc={(s) => {
      if (fs.at) forge.switchTo(s);
      else if (s !== src) fetchSource(s);
    }}
    onlevel={setLevel}
    ontests={(v) => (tests = v)}
    onfit={() => diagram?.fit()}
    onreset={() => (offsets = new Map())}
    onfile={(f) => f.text().then((t) => loadOpened(t, null))}
    ondir={openDirectory}
    onrepo={openForgeInput}
    {auth}
  />
  {#if project && !phase}
    <div class="flex items-center gap-3 border-b border-[var(--line)] px-3 py-1 text-[var(--muted)]" title={project.tooltip} data-project>
      {#if fs.at}
        <button
          class="flex shrink-0 items-center gap-2 border border-[var(--line)] px-2 py-0.5 text-[var(--fg)] hover:border-[var(--muted)] {fs.branchOpen ? 'bg-[var(--line)]' : ''}"
          onclick={forge.toggleBranches}
          title="switch to another branch"
          aria-haspopup="listbox"
          aria-expanded={fs.branchOpen}
          data-branch-toggle
        >
          <span class="text-[var(--muted)]">branch</span>
          {#if fs.branch}<span>{fs.branch}</span>{/if}
          <span class="text-[var(--muted)]" aria-hidden="true">▾</span>
        </button>
        <!-- The commit shown, or the two commits of the overlay: the control of the commit list. -->
        <button
          class="flex shrink-0 items-center gap-2 border border-[var(--line)] px-2 py-0.5 text-[var(--fg)] tabular-nums hover:border-[var(--muted)] {fs.listOpen ? 'bg-[var(--line)]' : ''}"
          onclick={forge.toggleList}
          title="pick another commit"
          aria-haspopup="listbox"
          aria-expanded={fs.listOpen}
          data-list-toggle
        >
          <span class="text-[var(--muted)]">commit</span>
          <span>{diff ? `${diff.base} → ${diff.head}` : shaOf(fs.at).slice(0, 7)}</span>
          <span class="text-[var(--muted)]" aria-hidden="true">▾</span>
        </button>
      {/if}
      <span class="min-w-0">{project.text}</span>
    </div>
  {/if}
  {#if fs.at && fs.branchOpen}
    <!-- A menu over the diagram, like the commit list. -->
    <div class="relative z-30">
      <div class="absolute inset-x-0 top-0 shadow-lg">
        <BranchList
          branches={fs.branches}
          status={fs.branchStatus}
          current={fs.branch}
          head={shaOf(fs.at)}
          onpick={forge.pickBranch}
        />
      </div>
    </div>
  {/if}
  {#if fs.at && fs.listOpen}
    {@const rows = fs.commits ? commitRows(fs.commits.list, fs.overlay?.range ?? null) : null}
    <!-- A menu over the diagram: it takes no height, so that the diagram does not move when it opens. -->
    <div class="relative z-30">
      <div class="absolute inset-x-0 top-0 shadow-lg">
        <CommitList
          {rows}
          hidden={fs.commits && rows ? fs.commits.list.length - rows.length : 0}
          status={fs.listStatus}
          more={fs.commits ? fs.commits.next !== null : false}
          onpick={forge.pick}
          onsha={forge.pickSha}
          onmore={forge.loadMore}
        />
      </div>
    </div>
  {/if}
  {#if diff && totals}
    <CommitStrip
      {diff}
      {totals}
      changesOnly={opts.changesOnly}
      onchanges={(v) => (opts.changesOnly = v)}
      together={opts.changesTogether}
      ontogether={(v) => {
        opts.changesTogether = v;
        fitNext = true;
      }}
      {files}
      onopen={() => {
        if (!tree) return;
        expanded = openChangedStep(tree, tests, expanded);
        unfolded = new Set();
        setFocus({ nodes: [], changes: true, neighbors: focus.neighbors });
      }}
      label={!fs.at}
    />
  {/if}
  {#if fs.at && fs.mode === 'commit' && fs.overlay && fs.repo}
    {#key `${fs.at}:${fs.overlay.range.base}`}
      <CommitContext source={forgeOf(fs.repo)} repo={fs.repo} range={fs.overlay.range} cache={contexts} />
    {/key}
  {/if}
  <EdgeBar {opts} onchange={(patch) => Object.assign(opts, patch)} drawn={paintedCount} total={drawn?.total ?? 0} dropped={drawn?.dropped ?? 0}
    hideImplied={drawn?.hideImplied ?? false}
    implied={drawn?.implied.length ?? 0}
  />
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
  <div class="relative flex min-h-0 flex-1">
    {#if phase || (tree && !laid && busy) || stepping}
      <!-- A load in progress covers the picture of the source before it; a failed load uncovers it again.
           The cover of a step shows late, so that a short step does not flash. -->
      <div
        class="absolute inset-0 z-10 flex items-center justify-center bg-[var(--bg)] {phase || !laid ? '' : 'late'}"
        data-loading
      >
        <Survey label={phase ?? 'layout'} />
      </div>
    {/if}
    {#if tree && view && laid}
      <div class="flex min-h-0 min-w-0 flex-1 flex-col">
        <Diagram
          bind:this={diagram}
          bind:hoverNode
          {tree}
          {view}
          layout={laid.layout}
          drawn={laid.drawn}
          onSelection={opts.onSelection}
          betweenOnly={opts.betweenOnly}
          commit={diff !== null}
          {changesOnly}
          {offsets}
          {expanded}
          {selected}
          {fileCounts}
          onselect={(id, add) => (selected = pick(selected, id, add))}
          onunfold={unfold}
          {shown}
          focused={focusActive(focus)}
          ontoggle={toggle}
          onoffset={(id, o) => (offsets = new Map(offsets).set(id, o))}
        />
        <SourcePanel {tree} selection={selected} {tests} kinds={opts.kinds} {between} read={readSource} />
      </div>
      <DetailPanel
        {tree}
        {view}
        lines={drawn ? [...drawn.edges, ...drawn.implied] : []}
        selected={selected.length === 1 ? selected[0] : null}
        selection={selected}
        step={selectionStep?.step ?? null}
        onstep={stepSelection}
        {tests}
        commit={diff !== null}
        {files}
        onselect={(id) => (selected = [id])}
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
          <p>No <code>graph.json</code> here. Open a project directory, or drop one anywhere on this page, to index it in this browser (Java, Rust and Clojure). Or give a repository of GitHub or of a Forgejo host (a private one with a token). Or open a graph file made by <code>cg index</code>.</p>
          <p class="text-[var(--muted)]">A local directory is not stored: a reload returns to this screen. A repository stays in the link; its token does not.</p>
          <div class="flex flex-wrap gap-2">
            <OpenButtons
              btn="px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]"
              onfile={(f) => f.text().then((t) => loadOpened(t, null))}
              ondir={openDirectory}
              onrepo={openForgeInput}
              {auth}
            />
          </div>
        </div>
      </div>
    {:else}
      <div class="flex-1"></div>
    {/if}
  </div>
</div>

<style>
  /* Opacity only: the browser can run it while the cut blocks the main thread. */
  .late {
    animation: late 0.15s 0.12s both;
  }
  @keyframes late {
    from {
      opacity: 0;
    }
  }
</style>
