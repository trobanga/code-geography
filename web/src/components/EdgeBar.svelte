<script lang="ts">
  import { MIN_COUNT_MAX, type EdgeOptions } from '../lib/edgeOptions';
  import { EDGE_KINDS } from '../lib/types';

  let {
    opts,
    drawn,
    total,
    onchange,
  }: {
    opts: EdgeOptions;
    drawn: number;
    total: number;
    onchange: (patch: Partial<EdgeOptions>) => void;
  } = $props();

  const flag = (key: 'onSelection' | 'lift' | 'mergeImports' | 'reduce') => (e: Event) =>
    onchange({ [key]: (e.currentTarget as HTMLInputElement).checked });
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1">
  <span class="text-[var(--muted)]">edges</span>
  <label class="flex items-center gap-1">
    <input type="checkbox" checked={opts.onSelection} onchange={flag('onSelection')} />on selection
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
    <label class="flex items-center gap-1">
      min
      <input
        type="number"
        min="1"
        max={MIN_COUNT_MAX}
        value={opts.min}
        class="w-10 border border-[var(--line)] bg-[var(--bg)] px-1"
        oninput={(e) => {
          const v = Number(e.currentTarget.value);
          if (Number.isInteger(v) && v >= 1 && v <= MIN_COUNT_MAX) onchange({ min: v });
        }}
      />
    </label>
  </span>
  <label class="flex items-center gap-1 border-l border-[var(--line)] pl-3">
    <input type="checkbox" checked={opts.lift} onchange={flag('lift')} />lift
  </label>
  <label class="flex items-center gap-1">
    <input type="checkbox" checked={opts.mergeImports} onchange={flag('mergeImports')} />merge imports
  </label>
  <label class="flex items-center gap-1">
    <input type="checkbox" checked={opts.reduce} onchange={flag('reduce')} />transitive reduction
  </label>
  <span class="ml-auto tabular-nums text-[var(--muted)]">lines: {drawn} / {total}</span>
</div>
