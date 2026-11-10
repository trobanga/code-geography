<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import DetailPanel from './components/DetailPanel.svelte';
  import Diagram from './components/Diagram.svelte';
  import EdgeBar from './components/EdgeBar.svelte';
  import Toolbar from './components/Toolbar.svelte';
  import type { Offset } from './lib/drag';
  import { DEFAULT_EDGE_OPTIONS, edgePipeline, painted, type Drawn, type EdgeOptions } from './lib/edgeOptions';
  import { toElkGraph } from './lib/elkGraph';
  import { buildTree, collapse, levelExpanded, splitByLevel, type Tree } from './lib/expansion';
  import { decodeHash, encodeHash } from './lib/hash';
  import { runLayout, type Layout } from './lib/layout';
  import type { Graph, Level, View } from './lib/types';
  import init, { Explorer } from './lib/wasm/cg_wasm.js';

  let error = $state<string | null>(null);
  let explorer = $state.raw<Explorer | null>(null);
  let tree = $state.raw<Tree | null>(null);
  let expanded = $state.raw<Set<string>>(new Set());
  let tests = $state(false);
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

  const hashState = $derived(tree ? splitByLevel(tree, expanded) : null);
  const activeLevel = $derived(hashState && hashState.open.length === 0 ? hashState.level : null);

  // O4 is part of the WASM view.
  const view = $derived(
    explorer && tree ? (JSON.parse(explorer.view([...expanded], tests, opts.mergeImports)) as View) : null,
  );
  // O2 → O3 → O5.
  const drawn = $derived(view && tree ? edgePipeline(view.edges, tree, opts) : null);
  const paintedCount = $derived(
    laid
      ? laid.layout.edges.filter((le) => painted(laid!.drawn.edges[le.index], opts.onSelection, selected, hoverNode))
          .length
      : 0,
  );

  async function load(text: string) {
    try {
      await init();
      const graph = JSON.parse(text) as Graph;
      const ex = new Explorer(text);
      explorer?.free();
      explorer = ex;
      tree = buildTree(graph.nodes);
      offsets = new Map();
      error = null;
      applyHash(location.hash);
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
    fitNext = true;
  }

  function setLevel(l: Level) {
    if (!tree) return;
    expanded = levelExpanded(tree, l);
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
    const h = encodeHash({ level: hashState.level, tests, open: hashState.open, edges: opts });
    if (h !== location.hash) history.replaceState(null, '', h);
    lastHash = h;
  });

  onMount(() => {
    const onHash = () => {
      if (location.hash !== lastHash) applyHash(location.hash);
    };
    window.addEventListener('hashchange', onHash);
    fetch('/graph.json')
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then(load)
      .catch((e) => (error = `cannot load /graph.json: ${e instanceof Error ? e.message : String(e)}`));
    return () => window.removeEventListener('hashchange', onHash);
  });
</script>

<div class="flex h-full flex-col">
  <Toolbar
    level={activeLevel}
    {tests}
    {busy}
    moved={offsets.size > 0}
    onlevel={setLevel}
    ontests={(v) => (tests = v)}
    onfit={() => diagram?.fit()}
    onreset={() => (offsets = new Map())}
    onfile={(f) => {
      selected = null;
      f.text().then(load);
    }}
  />
  <EdgeBar {opts} onchange={(patch) => Object.assign(opts, patch)} drawn={paintedCount} total={drawn?.total ?? 0} />
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
        {offsets}
        {expanded}
        {selected}
        onselect={(id) => (selected = id)}
        ontoggle={toggle}
        onoffset={(id, o) => (offsets = new Map(offsets).set(id, o))}
      />
      <DetailPanel {tree} {view} {selected} {tests} onselect={(id) => (selected = id)} />
    {:else}
      <div class="flex-1"></div>
    {/if}
  </div>
</div>
