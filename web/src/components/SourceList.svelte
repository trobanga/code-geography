<script lang="ts">
  import SourceCode from './SourceCode.svelte';
  import type { Tree } from '../lib/expansion';
  import { CHANGE_SIGN, changeColor, KIND_LETTER, kindColor } from '../lib/kinds';
  import { hasLines, PAGE, qualifier, type LineGroup, type ReadSource } from '../lib/source';

  let {
    tree,
    rows,
    read,
  }: {
    tree: Tree;
    /** The declarations, each with the targets of its edges; no targets for a plain list. */
    rows: readonly LineGroup[];
    /** Null: the graph has no texts, so no row opens. */
    read: ReadSource | null;
  } = $props();

  /** The one declaration that shows its lines. */
  let open = $state<string | null>(null);
  /** Rows shown; the list can hold thousands. */
  let limit = $state(PAGE);
  const nameOf = (id: string) => tree.label.get(id) ?? id;
</script>

{#each rows.slice(0, limit) as row (row.from)}
  {@const n = tree.byId.get(row.from)}
  {@const lines = read !== null && n !== undefined && hasLines(n) ? { node: n, read } : null}
  <div class="border-b border-[var(--line)]" data-source-row={row.from}>
    <button
      class="flex w-full flex-wrap items-baseline gap-x-1 px-3 py-0.5 text-left {lines ? 'hover:bg-[var(--bg)]' : 'cursor-default'}"
      disabled={!lines}
      aria-expanded={open === row.from}
      onclick={() => (open = open === row.from ? null : row.from)}
    >
      <span class="w-3 text-[var(--muted)]" aria-hidden="true">{lines ? (open === row.from ? '▾' : '▸') : ''}</span>
      {#if n?.change}
        <span class="w-2 font-bold" style="color: {changeColor(n.change)}" title={n.change}>{CHANGE_SIGN[n.change]}</span>
      {/if}
      {#if n}<span class="font-bold" style="color: {kindColor(n.kind)}">{KIND_LETTER[n.kind]}</span>{/if}
      <span class={n?.change === 'removed' ? 'line-through' : ''} title={row.from}
        ><span class="text-[var(--muted)]">{qualifier(tree, row.from)}</span>{nameOf(row.from)}</span
      >
      {#if row.targets.length > 0}
        <span class="text-[var(--muted)]" aria-hidden="true">→</span>
        {#each row.targets as t, i (t.id)}
          <span title="{t.id} ({t.kinds.join(', ')})"
            >{#if t.change}<span class="font-bold" style="color: {changeColor(t.change)}">{CHANGE_SIGN[t.change]}</span
              >{/if}<span class="text-[var(--muted)]">{qualifier(tree, t.id)}</span>{nameOf(t.id)}
            <span class="text-[11px] text-[var(--muted)]">{t.kinds.join(' ')}</span>{i + 1 < row.targets.length ? ',' : ''}</span
          >
        {/each}
      {/if}
    </button>
    {#if lines && open === row.from}
      <div class="overflow-x-auto border-t border-[var(--line)] bg-[var(--bg)]">
        <SourceCode node={lines.node} read={lines.read} />
      </div>
    {/if}
  </div>
{/each}
{#if rows.length > limit}
  <button class="px-3 py-1 text-[var(--muted)] hover:text-[var(--fg)]" onclick={() => (limit += PAGE)}>
    show {Math.min(PAGE, rows.length - limit)} more of {rows.length - limit}
  </button>
{/if}
