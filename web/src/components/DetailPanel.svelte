<script lang="ts">
  import { breakdown, edgesOf } from '../lib/detail';
  import type { Tree } from '../lib/expansion';
  import { KIND_LETTER, kindColor } from '../lib/kinds';
  import type { View, ViewEdge } from '../lib/types';

  let {
    tree,
    view,
    selected,
    tests,
    onselect,
  }: {
    tree: Tree;
    view: View;
    selected: string | null;
    tests: boolean;
    onselect: (id: string) => void;
  } = $props();

  const node = $derived(selected ? tree.byId.get(selected) : undefined);
  const childCount = $derived(
    selected
      ? (tree.children.get(selected) ?? []).filter((c) => tests || !tree.byId.get(c)?.test).length
      : 0,
  );
  const lists = $derived(selected ? edgesOf(view, selected) : { outgoing: [], incoming: [] });
  const nameOf = (id: string) => tree.label.get(id) ?? id;
</script>

{#snippet edgeList(title: string, edges: ViewEdge[], other: (e: ViewEdge) => string)}
  <div class="mt-3">
    <div class="text-[var(--muted)]">{title} ({edges.length})</div>
    {#each edges as e (e.from + '>' + e.to)}
      {@const id = other(e)}
      {@const k = tree.byId.get(id)?.kind}
      <button
        class="block w-full border-b border-[var(--line)] py-0.5 text-left hover:bg-[var(--bg)]"
        onclick={() => onselect(id)}
      >
        <div class="flex gap-1">
          {#if k}<span class="font-bold" style="color: {kindColor(k)}">{KIND_LETTER[k]}</span>{/if}
          <span class="truncate" title={id}>{nameOf(id)}</span>
          <span class="ml-auto tabular-nums">{e.count}</span>
        </div>
        <div class="pl-3 text-[11px] text-[var(--muted)]">{breakdown(e.kinds)}</div>
      </button>
    {/each}
  </div>
{/snippet}

<aside class="w-80 shrink-0 overflow-y-auto border-l border-[var(--line)] bg-[var(--panel)] p-3">
  {#if node}
    <div class="flex items-baseline gap-2">
      <span class="font-bold" style="color: {kindColor(node.kind)}">{node.kind}</span>
      {#if node.test}<span class="text-[var(--muted)]">test</span>{/if}
    </div>
    <div class="mt-1 text-sm font-bold break-all">{node.name}</div>
    <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 text-[11px]">
      <dt class="text-[var(--muted)]">id</dt>
      <dd class="break-all">{node.id}</dd>
      {#if node.file}
        <dt class="text-[var(--muted)]">file</dt>
        <dd class="break-all">
          {node.file}{#if node.span}:{node.span.start_line}-{node.span.end_line}{/if}
        </dd>
      {/if}
      <dt class="text-[var(--muted)]">children</dt>
      <dd>{childCount}</dd>
    </dl>
    {@render edgeList('outgoing', lists.outgoing, (e) => e.to)}
    {@render edgeList('incoming', lists.incoming, (e) => e.from)}
  {:else}
    <div class="text-[var(--muted)]">no selection</div>
  {/if}
</aside>
