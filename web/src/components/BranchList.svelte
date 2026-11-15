<script lang="ts">
  import type { Branch } from '../lib/forge';
  import { filterBranches } from '../lib/menuSearch';

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

  let query = $state('');
  let field: HTMLInputElement | undefined = $state();
  const found = $derived(branches ? filterBranches(branches, query) : []);

  $effect(() => field?.focus());

  const marked = (b: Branch) => (current !== null ? b.name === current : b.sha === head);
</script>

<div class="border-b border-[var(--line)] bg-[var(--panel)] tabular-nums" data-branches>
  <div class="flex items-baseline gap-3 px-3 pt-1">
    <input
      bind:this={field}
      bind:value={query}
      class="min-w-0 flex-1 border border-[var(--line)] bg-transparent px-1 py-0.5 focus:border-[var(--muted)] focus:outline-none"
      type="text"
      spellcheck="false"
      autocomplete="off"
      placeholder="filter by parts of the name"
      aria-label="filter the branches"
      data-branch-query
      onkeydown={(e) => {
        // Enter: the first branch of the list, which is the best match.
        if (e.key === 'Enter' && query.trim() !== '' && found.length > 0) onpick(found[0].name);
      }}
    />
    {#if branches && query.trim() !== ''}
      <span class="shrink-0 text-[var(--muted)]" data-branch-count>{found.length} of {branches.length}</span>
    {/if}
  </div>
  <div class="max-h-48 overflow-y-auto px-3 py-1" role="listbox" aria-label="branches">
    {#if status}
      <div class="text-[var(--muted)]">{status}</div>
    {/if}
    {#if branches}
      {#if found.length === 0}
        <div class="text-[var(--muted)]">{branches.length === 0 ? 'no branches' : `no match among the ${branches.length} branches`}</div>
      {:else}
        <ul>
          {#each found as b (b.name)}
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
</div>
