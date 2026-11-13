<script lang="ts">
  import type { Branch } from '../lib/forge';

  let {
    branches,
    status,
    current,
    head,
    onpick,
  }: {
    /** null: not listed yet. */
    branches: Branch[] | null;
    /** A message above the list, e.g. while it loads or after it failed. */
    status: string | null;
    /** The branch picked from this menu; null when the commit shown came another way. */
    current: string | null;
    /** The full sha of the commit shown; marks each branch at it when `current` is null. */
    head: string | null;
    onpick: (name: string) => void;
  } = $props();

  const marked = (b: Branch) => (current !== null ? b.name === current : b.sha === head);
</script>

<div class="max-h-48 overflow-y-auto border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1 tabular-nums" role="listbox" aria-label="branches" data-branches>
  {#if status}
    <div class="text-[var(--muted)]">{status}</div>
  {/if}
  {#if branches}
    {#if branches.length === 0}
      <div class="text-[var(--muted)]">no branches</div>
    {:else}
      <ul>
        {#each branches as b (b.name)}
          <li>
            <button
              class="flex w-full items-baseline gap-3 px-1 text-left hover:bg-[var(--line)] {marked(b) ? 'bg-[var(--line)] font-semibold' : ''}"
              role="option"
              aria-selected={marked(b)}
              data-branch={b.name}
              title={b.sha}
              onclick={() => onpick(b.name)}
            >
              <span class="w-3 shrink-0" aria-hidden="true">{marked(b) ? '●' : ''}</span>
              <span class="min-w-0 flex-1 truncate">{b.name}</span>
              <code class="shrink-0 text-[var(--muted)]">{b.sha.slice(0, 7)}</code>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>
