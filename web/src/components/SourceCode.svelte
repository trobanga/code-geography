<script lang="ts">
  import { CHANGE_SIGN, changeColor } from '../lib/kinds';
  import { hasLines, readInto, refsTitle, type ReadSource, type SourceLine, type SourceState } from '../lib/source';
  import type { GraphEdge, GraphNode } from '../lib/types';

  let {
    node,
    read,
    edges = [],
    nameOf = (id: string) => id,
  }: {
    /** A node with a file and a span, or a node without lines whose edges name the files. */
    node: GraphNode;
    read: ReadSource;
    /** The edges behind the line between the two selected boxes: their sites mark lines of the source (ADR 0032). */
    edges?: readonly GraphEdge[];
    /** The name of a target for the tooltip of a marked line. */
    nameOf?: (id: string) => string;
  } = $props();

  /** A change mark keeps its own background; a marked reference gets a light tint and room above and below when it is scrolled to. */
  function rowStyle(l: SourceLine): string {
    const background = l.mark
      ? `background: color-mix(in srgb, ${changeColor(l.mark)} 14%, transparent)`
      : l.refs
        ? 'background: color-mix(in srgb, var(--accent) 12%, transparent)'
        : '';
    return l.refs ? `${background}; scroll-margin-block: 4em` : background;
  }

  let got = $state.raw<SourceState>({ status: 'reading' });
  let root = $state<HTMLElement>();

  // A text is read when its lines are shown, not before. The cleanup drops the reply for another node.
  $effect(() => readInto(read, node, edges, (s) => (got = s)));

  // Once per read: the first marked line comes into view. Without a marked line nothing moves.
  $effect(() => {
    if (got.status === 'lines') root?.querySelector('[data-ref]')?.scrollIntoView({ block: 'nearest' });
  });
</script>

{#snippet header(file: string, own: boolean)}
  <div class="px-3 py-0.5 text-[11px] break-all text-[var(--muted)]" data-source-block={own ? 'own' : 'outside'}>
    {#if own && node.span}
      {file}:{node.span.start_line}-{node.span.end_line}
    {:else}
      {file} <span class="italic">references outside the declaration</span>
    {/if}
  </div>
{/snippet}

<div data-source-code bind:this={root}>
  {#if hasLines(node) && got.status !== 'lines'}{@render header(node.file, true)}{/if}
  {#if got.status === 'reading'}
    <div class="px-3 py-1 text-[var(--muted)]">reading</div>
  {:else if got.status === 'error'}
    <div class="px-3 py-1 text-[var(--accent)]">cannot show the source: {got.message}</div>
  {:else}
    {#each got.blocks as block, b (b)}
      {@render header(block.file, block.own)}
      <table class="min-w-full border-collapse leading-[1.5]">
        <tbody>
          {#each block.lines as l, i (i)}
            {@const ref = l.refs?.[0]}
            {#if l.gap}
              <tr data-gap>
                <td class="pr-2 pl-3 text-right text-[var(--muted)] select-none" colspan="4">⋮</td>
              </tr>
            {/if}
            <tr
              style={rowStyle(l)}
              data-mark={l.mark}
              data-ref={l.refs ? '' : undefined}
              title={l.refs ? refsTitle(l.refs, nameOf) : undefined}
            >
              <td class="pr-2 pl-3 text-right align-top text-[var(--muted)] tabular-nums select-none">{l.no}</td>
              <!-- The spans keep both columns as wide in a block without marks as in one with them. -->
              <td class="align-top font-bold select-none" style={l.mark ? `color: ${changeColor(l.mark)}` : ''}
                ><span class="inline-block w-3 text-center">{l.mark ? CHANGE_SIGN[l.mark] : ''}</span></td
              >
              <td
                class="pr-1 align-top font-bold select-none"
                style="color: {ref?.change ? changeColor(ref.change) : 'var(--accent)'}"
                aria-hidden="true"><span class="inline-block w-3 text-center">{l.refs ? '→' : ''}</span></td
              >
              <td class="w-full pr-3 whitespace-pre">{l.text}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/each}
  {/if}
</div>
