<script lang="ts">
  import { CHANGE_SIGN, changeColor } from '../lib/kinds';
  import { readInto, type ReadSource, type SourceState } from '../lib/source';
  import type { GraphNode } from '../lib/types';

  let {
    node,
    read,
  }: {
    /** A node with a file and a span. */
    node: GraphNode;
    read: ReadSource;
  } = $props();

  let got = $state.raw<SourceState>({ status: 'reading' });

  // A text is read when its lines are shown, not before. The cleanup drops the reply for another node.
  $effect(() => readInto(read, node, (s) => (got = s)));
</script>

<div data-source-code>
  <div class="px-3 py-0.5 text-[11px] break-all text-[var(--muted)]">
    {node.file}{#if node.span}:{node.span.start_line}-{node.span.end_line}{/if}
  </div>
  {#if got.status === 'reading'}
    <div class="px-3 py-1 text-[var(--muted)]">reading</div>
  {:else if got.status === 'error'}
    <div class="px-3 py-1 text-[var(--accent)]">cannot show the source: {got.message}</div>
  {:else}
    <table class="min-w-full border-collapse leading-[1.5]">
      <tbody>
        {#each got.source.lines as l, i (i)}
          <tr
            style={l.mark ? `background: color-mix(in srgb, ${changeColor(l.mark)} 14%, transparent)` : ''}
            data-mark={l.mark}
          >
            <td class="pr-2 pl-3 text-right align-top text-[var(--muted)] tabular-nums select-none">{l.no}</td>
            <td class="w-3 align-top font-bold select-none" style={l.mark ? `color: ${changeColor(l.mark)}` : ''}
              >{l.mark ? CHANGE_SIGN[l.mark] : ''}</td
            >
            <td class="w-full pr-3 whitespace-pre">{l.text}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
