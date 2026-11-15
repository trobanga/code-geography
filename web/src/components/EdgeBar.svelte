<script lang="ts">
  import { linesText, type EdgeOptions } from '../lib/edgeOptions';
  import { EDGE_KINDS } from '../lib/types';

  let {
    opts,
    drawn,
    total,
    dropped,
    hideImplied,
    implied,
    onchange,
  }: {
    opts: EdgeOptions;
    drawn: number;
    total: number;
    /** Placeholder lines that lift dropped. */
    dropped: number;
    /** Whether hide implied is in effect now (by choice or by auto). */
    hideImplied: boolean;
    /** Lines hidden as implied. */
    implied: number;
    onchange: (patch: Partial<EdgeOptions>) => void;
  } = $props();

  const flag = (key: 'onSelection' | 'lift' | 'betweenOnly') => (e: Event) =>
    onchange({ [key]: (e.currentTarget as HTMLInputElement).checked });
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1">
  <span class="text-[var(--muted)]">edges</span>
  <label class="flex items-center gap-1">
    <input type="checkbox" checked={opts.onSelection} onchange={flag('onSelection')} />on selection
  </label>
  <label class="flex items-center gap-1" title="With two or more selected boxes (Shift+click): show the lines between them only. Off: the lines to other boxes stay, weaker.">
    <input type="checkbox" checked={opts.betweenOnly} onchange={flag('betweenOnly')} />between selected
  </label>
  <span class="flex items-center gap-2 border-l border-[var(--line)] pl-3">
    {#each EDGE_KINDS as k (k)}
      <label class="flex items-center gap-1">
        <input
          type="checkbox"
          checked={opts.kinds[k]}
          onchange={(e) => onchange({ kinds: { ...opts.kinds, [k]: e.currentTarget.checked } })}
        />{k}
      </label>
    {/each}
  </span>
  <label class="flex items-center gap-1 border-l border-[var(--line)] pl-3">
    <input type="checkbox" checked={opts.lift} onchange={flag('lift')} />lift
  </label>
  <label
    class="flex items-center gap-1 border-l border-[var(--line)] pl-3"
    title="Hides a line A→C when a path A→B→C is drawn. The side panel lists all."
  >
    <input type="checkbox" checked={hideImplied} onchange={(e) => onchange({ implied: e.currentTarget.checked })} />hide implied
    {#if opts.implied === null}<span class="text-[var(--muted)]">(auto)</span>{/if}
  </label>
  <span class="ml-auto tabular-nums text-[var(--muted)]">{linesText(drawn, total, dropped, implied)}</span>
</div>
