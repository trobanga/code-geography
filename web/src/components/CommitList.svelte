<script lang="ts">
  import type { CommitRow } from '../lib/forgeDiff';

  let {
    rows,
    hidden = 0,
    status,
    onpick,
  }: {
    /** Commits of update bots (Renovate, Dependabot) that the list leaves out. */
    hidden?: number;
    /** null: not loaded yet. */
    rows: CommitRow[] | null;
    /** A message instead of the list, e.g. while it loads or after it failed. */
    status: string | null;
    onpick: (sha: string) => void;
  } = $props();
</script>

<div class="max-h-48 overflow-y-auto border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1 tabular-nums" data-commits>
  {#if status}
    <div class="text-[var(--muted)]">{status}</div>
  {:else if rows}
    <div class="text-[var(--muted)]">
      pick a commit to see its overlay against its first parent{#if hidden > 0}<span data-hidden-commits>
          · {hidden} commit{hidden === 1 ? '' : 's'} of dependency updates by a bot not shown</span
        >{/if}
    </div>
    <ul>
      {#each rows as r (r.sha)}
        <li>
          <button
            class="flex w-full items-baseline gap-3 px-1 text-left hover:bg-[var(--line)] {r.current ? 'bg-[var(--line)] font-semibold' : ''}"
            data-commit={r.sha}
            title={r.sha}
            onclick={() => onpick(r.sha)}
          >
            <code class="shrink-0">{r.short}</code>
            <span class="min-w-0 flex-1 truncate">{r.title}</span>
            <span class="shrink-0 text-[var(--muted)]">{r.author}</span>
            <span class="shrink-0 text-[var(--muted)]">{r.day}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
