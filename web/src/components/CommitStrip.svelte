<script lang="ts">
  import type { CommitTotals } from '../lib/commit';

  let {
    diff,
    totals,
    changesOnly,
    onchanges,
    onopen,
  }: {
    diff: { base: string; head: string };
    totals: CommitTotals;
    changesOnly: boolean;
    onchanges: (v: boolean) => void;
    onopen: () => void;
  } = $props();

  const btn = 'px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]';
  const hidden = $derived(totals.hidden.nodes + totals.hidden.edges > 0);
  /** Zero counts stay in place but recede. */
  const tint = (n: number, k: string) => (n > 0 ? `color: var(--c-${k})` : 'color: var(--muted)');
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1 tabular-nums">
  <span class="text-[var(--muted)]">commit</span>
  <span>{diff.base} → {diff.head}</span>
  <span class="border-l border-[var(--line)] pl-3">
    <span class="text-[var(--muted)]">nodes</span>
    <span style={tint(totals.nodes.added, 'added')}>+{totals.nodes.added}</span>
    <span style={tint(totals.nodes.removed, 'removed')}>−{totals.nodes.removed}</span>
    <span style={tint(totals.nodes.modified, 'modified')}>~{totals.nodes.modified}</span>
  </span>
  <span>
    <span class="text-[var(--muted)]">edges</span>
    <span style={tint(totals.edges.added, 'added')}>+{totals.edges.added}</span>
    <span style={tint(totals.edges.removed, 'removed')}>−{totals.edges.removed}</span>
  </span>
  {#if hidden}
    <span class="text-[var(--muted)]"
      >{totals.hidden.nodes} nodes, {totals.hidden.edges} edges more in test code</span
    >
  {/if}
  <label class="flex items-center gap-1 border-l border-[var(--line)] pl-3">
    <input type="checkbox" checked={changesOnly} onchange={(e) => onchanges(e.currentTarget.checked)} />changes only
  </label>
  <button class={btn} onclick={onopen}>open changed</button>
</div>
