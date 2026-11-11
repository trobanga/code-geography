<script lang="ts">
  import { changeLabel, markedByModule } from '../lib/commit';
  import { breakdown, edgesOf } from '../lib/detail';
  import type { Tree } from '../lib/expansion';
  import type { ChangedFiles, FileGroup } from '../lib/files';
  import { CHANGE_SIGN, changeColor, KIND_LETTER, kindColor } from '../lib/kinds';
  import type { View, ViewEdge } from '../lib/types';

  let {
    tree,
    view,
    selected,
    tests,
    commit,
    files,
    onselect,
    onreveal,
    onfocus,
  }: {
    tree: Tree;
    view: View;
    selected: string | null;
    tests: boolean;
    commit: boolean;
    files: ChangedFiles | null;
    onselect: (id: string) => void;
    /** Select a node that may be hidden: open the containers on the path to it. */
    onreveal: (id: string) => void;
    /** Focus on the node and all that it touches. */
    onfocus: (id: string) => void;
  } = $props();

  const node = $derived(selected ? tree.byId.get(selected) : undefined);
  const childCount = $derived(
    selected
      ? (tree.children.get(selected) ?? []).filter((c) => tests || !tree.byId.get(c)?.test).length
      : 0,
  );
  const lists = $derived(selected ? edgesOf(view, selected) : { outgoing: [], incoming: [] });
  const nameOf = (id: string) => tree.label.get(id) ?? id;
  const marked = $derived(commit ? markedByModule(tree, tests, selected) : []);
  const btn = 'px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]';
  /** Files of the selected module, or all files without a selection. */
  const fileGroups = $derived(
    !files ? [] : selected === null ? files.groups : files.groups.filter((g) => g.module === selected),
  );
  const fileCount = $derived(fileGroups.reduce((a, g) => a + g.files.length, 0));
  const unexplainedSource = $derived(fileGroups.some((g) => g.files.some((f) => f.source && !f.explained)));
</script>

{#snippet markedList(groups: { module: string; nodes: string[] }[])}
  {#each groups as grp (grp.module)}
    <div class="mt-2 text-[var(--muted)]">{nameOf(grp.module)}</div>
    {#each grp.nodes as id (id)}
      {@const n = tree.byId.get(id)!}
      <button
        class="flex w-full gap-1 border-b border-[var(--line)] py-0.5 text-left hover:bg-[var(--bg)]"
        onclick={() => onreveal(id)}
      >
        <span class="w-2 font-bold" style="color: {changeColor(n.change!)}" title={n.change}>{CHANGE_SIGN[n.change!]}</span>
        <span class="font-bold" style="color: {kindColor(n.kind)}">{KIND_LETTER[n.kind]}</span>
        <span class="truncate {n.change === 'removed' ? 'line-through' : ''}" title={id}
          >{#if (n.kind === 'function' || n.kind === 'value') && n.parent}<span class="text-[var(--muted)]">{nameOf(n.parent)}.</span
            >{/if}{nameOf(id)}</span
        >
      </button>
    {/each}
  {/each}
{/snippet}

{#snippet fileList(groups: FileGroup[], heading: boolean)}
  {#each groups as grp (grp.module)}
    {#if heading}
      <div class="mt-2 text-[var(--muted)]">{grp.module === null ? 'outside modules' : nameOf(grp.module)}</div>
    {/if}
    {#each grp.files as f (f.path)}
      <div
        class="flex gap-1 border-b border-[var(--line)] py-0.5"
        title="{f.path}{f.explained
          ? ' (changed nodes below)'
          : f.source
            ? ' (change outside declarations)'
            : ''}"
      >
        <span class="w-2 font-bold" style="color: {changeColor(f.change)}">{CHANGE_SIGN[f.change]}</span>
        <span
          class="truncate {f.change === 'removed' ? 'line-through' : ''} {f.explained || !f.source
            ? ''
            : 'italic'}"
          style={f.explained ? '' : 'color: var(--muted)'}
          >{f.rel.slice(f.rel.lastIndexOf('/') + 1)}{#if f.rel.includes('/')}<span class="text-[var(--muted)]">{' '}{f.rel.slice(0, f.rel.lastIndexOf('/'))}</span
            >{/if}</span
        >
        {#if f.source && !f.explained}<span class="ml-auto shrink-0 text-[var(--muted)]">no node</span>{/if}
      </div>
    {/each}
  {/each}
  {#if unexplainedSource}
    <div class="mt-1 text-[11px] text-[var(--muted)]">
      "no node": a source file whose change no declaration shows (imports, comments, formatting outside declarations).
    </div>
  {/if}
{/snippet}

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
          {#if commit && changeLabel(e)}
            <span class="ml-auto font-bold tabular-nums" style="color: var(--c-{e.added >= e.removed ? 'added' : 'removed'})"
              >{changeLabel(e)}</span
            >
            <span class="tabular-nums">{e.count}</span>
          {:else}
            <span class="ml-auto tabular-nums">{e.count}</span>
          {/if}
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
      {#if commit && node.change}
        <span class="font-bold" style="color: {changeColor(node.change)}">{CHANGE_SIGN[node.change]} {node.change}</span>
      {/if}
    </div>
    <div class="mt-1 text-sm font-bold break-all">{node.name}</div>
    <button class="mt-2 {btn}" onclick={() => onfocus(node.id)}>focus on this</button>
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
    {#if fileCount > 0}
      <div class="mt-3 text-[var(--muted)]">changed files ({fileCount})</div>
      {@render fileList(fileGroups, false)}
    {/if}
    {#if marked.length > 0}
      <div class="mt-3 text-[var(--muted)]">changes below ({marked.reduce((a, g) => a + g.nodes.length, 0)})</div>
      {@render markedList(marked)}
    {/if}
  {:else if commit}
    <div class="text-[var(--muted)]">changed ({marked.reduce((a, g) => a + g.nodes.length, 0)})</div>
    {@render markedList(marked)}
    {#if fileCount > 0}
      <div class="mt-3 text-[var(--muted)]">changed files ({fileCount})</div>
      {@render fileList(fileGroups, true)}
    {/if}
  {:else}
    <div class="text-[var(--muted)]">no selection</div>
  {/if}
</aside>
