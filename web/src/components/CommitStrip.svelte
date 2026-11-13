<script lang="ts">
  import type { CommitTotals } from '../lib/commit';
  import type { ChangedFiles } from '../lib/files';

  let {
    diff,
    totals,
    files,
    changesOnly,
    onchanges,
    onopen,
    label = true,
  }: {
    diff: { base: string; head: string };
    totals: CommitTotals;
    files: ChangedFiles | null;
    changesOnly: boolean;
    onchanges: (v: boolean) => void;
    onopen: () => void;
    /** False: the line above names the commit (a repository of a forge), so this strip holds the counts only. */
    label?: boolean;
  } = $props();

  const btn = 'px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]';
  const hiddenFiles = $derived(files?.hidden ?? 0);
  const hidden = $derived(totals.hidden.nodes + totals.hidden.edges + hiddenFiles > 0);
  /** Zero counts stay in place but recede. */
  const tint = (n: number, k: string) => (n > 0 ? `color: var(--c-${k})` : 'color: var(--muted)');
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1 tabular-nums">
  {#if label}
    <span class="text-[var(--muted)]">commit</span>
    <span>{diff.base} → {diff.head}</span>
  {/if}
  <span class={label ? 'border-l border-[var(--line)] pl-3' : ''}>
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
  {#if files}
    <span>
      <span class="text-[var(--muted)]">files</span>
      <span style={tint(files.totals.added, 'added')}>+{files.totals.added}</span>
      <span style={tint(files.totals.removed, 'removed')}>−{files.totals.removed}</span>
      <span style={tint(files.totals.modified, 'modified')}>~{files.totals.modified}</span>
    </span>
  {/if}
  {#if hidden}
    <span class="text-[var(--muted)]"
      >{totals.hidden.nodes} nodes, {totals.hidden.edges} edges{files ? `, ${hiddenFiles} files` : ''} more in test code</span
    >
  {/if}
  <label class="flex items-center gap-1 border-l border-[var(--line)] pl-3">
    <input type="checkbox" checked={changesOnly} onchange={(e) => onchanges(e.currentTarget.checked)} />changes only
  </label>
  <button class={btn} onclick={onopen} title="Opens down to the changed types. A second click opens the changed types also.">open changed</button>
</div>
