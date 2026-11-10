<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
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
  import { commitTotals, openChanged, pathToOpen } from './lib/commit';
  import { changedFiles } from './lib/files';
  import { focusActive, focusOn, NO_FOCUS, type Focus } from './lib/focus';
  import {
    buildTree,
    collapse,
    deeper,
    levelExpanded,
    mustConfirm,
    splitByLevel,
    stepBack,
    stepForward,
    type DepthStep,
    type Tree,
  } from './lib/expansion';
  import { depthKey } from './lib/keys';
  import { decodeHash, encodeHash, freshHash, type Source } from './lib/hash';
  import { indexInWorker } from './lib/indexProject';
  import { inputError, summarize, type Summary } from './lib/project';
  import { collectDir, fromEntry, type Collect, type DirEntry } from './lib/walk';
  import { runLayout, type Layout } from './lib/layout';
  import type { Graph, Level, View } from './lib/types';
  import init, { Explorer, isInput } from './lib/wasm/cg_wasm.js';

  // An import that a use covers is a duplicate, so the view never counts it (ADR 13).
  const MERGE_IMPORTS = true;

  let error = $state<string | null>(null);
  let graph = $state.raw<Graph | null>(null);
  /** null: a file or a directory opened from disk. */
  let src = $state<Source | null>('graph');
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
  const hashState = $derived(tree ? splitByLevel(tree, expanded, shown) : null);
  const activeLevel = $derived(hashState && hashState.open.length === 0 ? hashState.level : null);
  const partialLevel = $derived(
    hashState && hashState.open.length > 0 ? { level: hashState.level, extra: hashState.open.length } : null,
  );

  // O4 is part of the WASM view.
  // Without a focus the plain cut; with one the focus cut of ADR 0011.
  function cut(ex: Explorer, open: ReadonlySet<string>): View {
    const f = focus;
    const ids = [...open];
    const json = focusActive(f)
      ? ex.focusView(ids, tests, MERGE_IMPORTS, f.nodes, f.changes && graph?.diff != null, f.neighbors, f.unfolded)
      : ex.view(ids, tests, MERGE_IMPORTS);
    return JSON.parse(json) as View;
  }
  const view = $derived(explorer && tree ? cut(explorer, expanded) : null);
  /** What "+" would give; equal to `expanded` when nothing visible can open. */
  const deeperSet = $derived(view && tree ? deeper(tree, view.expandable, expanded, shown) : expanded);
  /** "+" history, so that "−" right after it returns to the same set (see `stepBack`). */
  let steps: DepthStep[] = [];
  /** A "+" that waits for a second click, valid only for the set it was computed for. */
  let asked = $state.raw<{ to: Set<string>; boxes: number } | null>(null);
  const askBoxes = $derived(asked && asked.to === deeperSet ? asked.boxes : null);
  // O2 → O3.
  const drawn = $derived(view && tree ? edgePipeline(view.edges, tree, opts) : null);
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
    expanded = new Set([...levelExpanded(tree, s.level), ...s.open]);
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
      selected = null;
      project = null;
      await load(text);
    } catch (e) {
      error = `cannot load /${s}.json: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** Replaces the graph by one opened from disk: nothing open, no focus, no selection. */
  async function loadOpened(text: string, summary: Summary | null) {
    const hash = freshHash(location.hash);
    selected = null;
    src = null;
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
    expanded = levelExpanded(tree, l);
    fitNext = true;
  }

  function stepDeeper() {
    if (!explorer || deeperSet.size === expanded.size) return;
    const next = deeperSet;
    // The count comes from the WASM cut of the next set, not from a copy of the cut.
    const boxes = askBoxes ?? cut(explorer, next).nodes.length;
    if (mustConfirm(boxes, askBoxes !== null)) {
      asked = { to: next, boxes };
      return;
    }
    asked = null;
    steps = stepForward(steps, expanded, next);
    expanded = next;
    fitNext = true;
  }

  function stepShallower() {
    if (!tree || expanded.size === 0) return;
    const r = stepBack(tree, steps, expanded);
    steps = r.steps;
    expanded = r.expanded;
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
    runLayout(toElkGraph({ ...v, edges: d.edges }, tree))
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
      level: hashState.level,
      tests,
      open: hashState.open,
      edges: opts,
      focus: focusActive(focus) ? focus : undefined,
    });
    if (h !== location.hash) history.replaceState(null, '', h);
    lastHash = h;
  });

  onMount(() => {
    const onHash = () => {
      if (location.hash === lastHash) return;
      const s = decodeHash(location.hash).src ?? 'graph';
      if (s !== src) fetchSource(s);
      else applyHash(location.hash);
    };
    window.addEventListener('hashchange', onHash);
    // The dev and preview servers answer unknown paths with index.html, so check the type.
    fetch('/commit.json', { method: 'HEAD' })
      .then((r) => (hasCommit = r.ok && !!r.headers.get('content-type')?.includes('json')))
      .catch(() => (hasCommit = false));
    fetchSource(decodeHash(location.hash).src ?? 'graph');
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
      canDeeper: deeperSet.size > expanded.size,
      canShallower: expanded.size > 0,
      ask: askBoxes,
      ondeeper: stepDeeper,
      onshallower: stepShallower,
    }}
    {tests}
    {busy}
    moved={offsets.size > 0}
    {src}
    {hasCommit}
    onsrc={(s) => {
      if (s !== src) fetchSource(s);
    }}
    onlevel={setLevel}
    ontests={(v) => (tests = v)}
    onfit={() => diagram?.fit()}
    onreset={() => (offsets = new Map())}
    onfile={(f) => f.text().then((t) => loadOpened(t, null))}
    ondir={openDirectory}
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
        setFocus({ nodes: [], changes: true, neighbors: focus.neighbors, unfolded: [] });
      }}
    />
  {/if}
  {#if tree && focusActive(focus)}
    <FocusBar
      {tree}
      {focus}
      rest={view?.rest}
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
        onunfold={(c) => (focus = { ...focus, unfolded: [...focus.unfolded, c] })}
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
          setFocus(r.focus);
        }}
      />
    {:else if missing}
      <div class="flex flex-1 items-center justify-center">
        <div class="flex max-w-md flex-col gap-3" data-start>
          <p>No <code>graph.json</code> here. Open a project directory, or drop one anywhere on this page, to index it in this browser (Java and Rust). Or open a graph file made by <code>cg index</code>.</p>
          <p class="text-[var(--muted)]">Nothing is stored: a reload returns to this screen.</p>
          <div class="flex gap-2">
            <OpenButtons
              btn="px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]"
              onfile={(f) => f.text().then((t) => loadOpened(t, null))}
              ondir={openDirectory}
            />
          </div>
        </div>
      </div>
    {:else}
      <div class="flex-1"></div>
    {/if}
  </div>
</div>
