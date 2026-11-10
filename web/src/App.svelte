<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import DetailPanel from './components/DetailPanel.svelte';
  import Diagram from './components/Diagram.svelte';
  import Toolbar from './components/Toolbar.svelte';
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
  let selected = $state<string | null>(null);
  let view = $state.raw<View | null>(null);
  let layout = $state.raw<Layout | null>(null);
  let busy = $state(false);
  let fitNext = true;
  let seq = 0;
  let lastHash = '';
  let diagram: ReturnType<typeof Diagram> | undefined = $state();

  const hashState = $derived(tree ? splitByLevel(tree, expanded) : null);
  const activeLevel = $derived(hashState && hashState.open.length === 0 ? hashState.level : null);

  async function load(text: string) {
    try {
      await init();
      const graph = JSON.parse(text) as Graph;
      const ex = new Explorer(text);
      explorer?.free();
      explorer = ex;
      tree = buildTree(graph.nodes);
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

  // View from the wasm cut; layout from ELK in a worker. Stale results are dropped.
  $effect(() => {
    if (!explorer || !tree) return;
    const v = JSON.parse(explorer.view([...expanded], tests, false)) as View;
    view = v;
    untrack(() => {
      if (selected && !v.nodes.includes(selected)) selected = null;
    });
    const my = ++seq;
    busy = true;
    const t0 = performance.now();
    runLayout(toElkGraph(v, tree))
      .then(async (l) => {
        if (my !== seq) return;
        console.info(`layout ${v.nodes.length} nodes ${l.edges.length} edges ${Math.round(performance.now() - t0)} ms`);
        layout = l;
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
    const h = encodeHash({ level: hashState.level, tests, open: hashState.open });
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
    onlevel={setLevel}
    ontests={(v) => (tests = v)}
    onfit={() => diagram?.fit()}
    onfile={(f) => {
      selected = null;
      f.text().then(load);
    }}
  />
  {#if error}
    <div class="border-b border-[var(--line)] px-3 py-1 text-[var(--accent)]">{error}</div>
  {/if}
  <div class="flex min-h-0 flex-1">
    {#if tree && view && layout}
      <Diagram
        bind:this={diagram}
        {tree}
        {view}
        {layout}
        {expanded}
        {selected}
        onselect={(id) => (selected = id)}
        ontoggle={toggle}
      />
      <DetailPanel {tree} {view} {selected} {tests} onselect={(id) => (selected = id)} />
    {:else}
      <div class="flex-1"></div>
    {/if}
  </div>
</div>
