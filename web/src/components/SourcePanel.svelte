<script lang="ts">
  import SourceCode from './SourceCode.svelte';
  import SourceList from './SourceList.svelte';
  import type { KindSwitch } from '../lib/edgeOptions';
  import type { Tree } from '../lib/expansion';
  import { kindColor } from '../lib/kinds';
  import { counted, lineGroups, lineSummary, qualifier, sourceView, type LineGroup, type ReadSource } from '../lib/source';
  import type { GraphEdge } from '../lib/types';

  let {
    tree,
    selection,
    tests,
    kinds,
    between,
    read,
  }: {
    tree: Tree;
    /** The selected boxes. One or two give a source view (ADR 0031). */
    selection: readonly string[];
    tests: boolean;
    /** The edge kinds that the picture draws. */
    kinds: KindSwitch;
    /** The edges behind the line from one box to another, from the graph in WASM. */
    between: (from: string, to: string) => GraphEdge[];
    /** Reads the lines of a node. Null: the graph has no texts (a local graph, `/commit.json`). */
    read: ReadSource | null;
  } = $props();

  /** The developer closed the panel; a selection does not open it again. */
  let hidden = $state(false);
  const shown = $derived(sourceView(tree, selection, tests, between));
  // On top of `shown`, so that a switch of an edge kind does not ask WASM for the edges again.
  const pair = $derived(
    shown?.kind === 'pair' ? { there: lineGroups(shown.there, kinds), back: lineGroups(shown.back, kinds) } : null,
  );
  const nameOf = (id: string) => tree.label.get(id) ?? id;
  /** A view with rows takes its part of the height also without texts; lines need the texts. */
  const tall = $derived(shown !== null && !hidden && (read !== null || shown.kind !== 'code'));
</script>

{#snippet direction(from: string, to: string, groups: LineGroup[])}
  <div class="border-b border-[var(--line)] px-3 py-1 text-[var(--muted)]" data-source-direction>
    <span class="text-[var(--fg)]">{nameOf(from)}</span> → <span class="text-[var(--fg)]">{nameOf(to)}</span>:
    {lineSummary(groups)}
  </div>
  <SourceList {tree} rows={groups} {read} />
{/snippet}

{#if shown}
  <section
    class="flex min-h-0 shrink-0 flex-col border-t border-[var(--line)] bg-[var(--panel)] {tall ? 'h-2/5' : ''}"
    data-source
  >
    <header class="flex items-baseline gap-2 border-b border-[var(--line)] px-3 py-1">
      <span class="text-[var(--muted)]">source</span>
      {#if shown.kind === 'code'}
        {@const n = tree.byId.get(shown.node)!}
        <span class="font-bold" style="color: {kindColor(n.kind)}">{n.kind}</span>
        <span class="min-w-0 truncate" title={n.id}
          ><span class="text-[var(--muted)]">{qualifier(tree, n.id)}</span>{nameOf(n.id)}</span
        >
      {:else if shown.kind === 'list'}
        <span class="min-w-0 truncate">{counted(shown.rows.length, 'declaration')} of {nameOf(shown.of)}</span>
      {:else if pair}
        <span class="min-w-0 truncate"
          >{pair.there.length + pair.back.length === 0 ? 'no line between' : 'the line between'}
          {nameOf(shown.a)} and {nameOf(shown.b)}</span
        >
      {/if}
      {#if read === null}
        <span class="text-[var(--muted)]" data-source-missing
          >not available: only a repository from a forge or a local directory has the texts</span
        >
      {/if}
      <button
        class="ml-auto shrink-0 border border-[var(--line)] px-2 hover:border-[var(--muted)]"
        onclick={() => (hidden = !hidden)}
        aria-expanded={!hidden}>{hidden ? 'show' : 'hide'}</button
      >
    </header>
    {#if tall}
      <!-- A new selection starts with every declaration closed. -->
      {#key selection.join('\u0000')}
        <div class="min-h-0 flex-1 overflow-auto">
          {#if shown.kind === 'code'}
            {#if read}<SourceCode node={tree.byId.get(shown.node)!} {read} />{/if}
          {:else if shown.kind === 'list'}
            <SourceList {tree} rows={shown.rows} {read} />
          {:else if pair}
            {@render direction(shown.a, shown.b, pair.there)}
            {@render direction(shown.b, shown.a, pair.back)}
          {/if}
        </div>
      {/key}
    {/if}
  </section>
{/if}
