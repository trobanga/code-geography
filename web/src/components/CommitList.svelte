<script lang="ts">
  import type { CommitRow } from '../lib/forgeDiff';
  import { filterCommits, rangeOf, shaOf } from '../lib/menuSearch';

  let {
    rows,
    hidden = 0,
    status,
    more = false,
    onpick,
    onsha,
    onmore,
    worktree,
    onrange,
    asks = 'asks the forge',
  }: {
    /** Commits of update bots (Renovate, Dependabot) that the list leaves out. */
    hidden?: number;
    /** null: not loaded yet. */
    rows: CommitRow[] | null;
    /** A message above the list, e.g. while it loads or after it failed. */
    status: string | null;
    /** The history holds commits after those of the list. */
    more?: boolean;
    onpick: (sha: string) => void;
    /** A full or short sha from the text field that no row of the list has. */
    onsha: (sha: string) => void;
    onmore: () => void;
    /** A first row for the uncommitted changes of a local repository, shown while the text field is empty. */
    worktree?: { current: boolean; onpick: () => void };
    /** A range `A..B` from the text field, for a repository that can compare any two commits. */
    onrange?: (base: string, head: string) => void;
    /** How the end of the hint for a sha that is not listed goes on. */
    asks?: string;
  } = $props();

  let query = $state('');
  let field: HTMLInputElement | undefined = $state();
  const found = $derived(rows ? filterCommits(rows, query) : []);
  const sha = $derived(shaOf(query));
  /** The sha of the text field, if no listed commit has it: the forge is asked for it. */
  const other = $derived(sha !== null && found.length === 0 ? sha : null);
  /** The text field as a range `A..B`, if `onrange` takes one. */
  const range = $derived(onrange ? rangeOf(query) : null);

  $effect(() => field?.focus());

  /** Enter: the commit of the pasted sha, or the only row left. */
  function enter() {
    if (range) onrange?.(range.base, range.head);
    else if (other) onsha(other);
    else if (found.length === 1) onpick(found[0].sha);
  }
</script>

<div class="border-b border-[var(--line)] bg-[var(--panel)] tabular-nums" data-commits>
  <div class="flex items-baseline gap-3 px-3 pt-1">
    <input
      bind:this={field}
      bind:value={query}
      class="min-w-0 flex-1 border border-[var(--line)] bg-transparent px-1 py-0.5 focus:border-[var(--muted)] focus:outline-none"
      type="text"
      spellcheck="false"
      autocomplete="off"
      placeholder="filter by title, author or day, or paste a sha"
      aria-label="filter the commits, or paste a sha"
      data-commit-query
      onkeydown={(e) => {
        if (e.key === 'Enter') enter();
      }}
    />
    {#if rows && query.trim() !== ''}
      <span class="shrink-0 text-[var(--muted)]" data-commit-count>{found.length} of {rows.length}</span>
    {/if}
  </div>
  <div class="max-h-48 overflow-y-auto px-3 py-1">
    {#if status}
      <div class="text-[var(--muted)]" data-commit-status>{status}</div>
    {/if}
    {#if worktree && query.trim() === ''}
      <button
        class="flex w-full items-baseline gap-3 px-1 text-left hover:bg-[var(--line)] {worktree.current ? 'bg-[var(--line)] font-semibold' : ''}"
        data-worktree
        onclick={worktree.onpick}
      >
        <span class="min-w-0 flex-1 truncate">uncommitted changes</span>
        <span class="shrink-0 text-[var(--muted)]">the working tree against HEAD</span>
      </button>
    {/if}
    {#if rows}
      <div class="text-[var(--muted)]">
        pick a commit to see its overlay against its first parent{#if hidden > 0}<span class="ml-2" data-hidden-commits>
            · {hidden} commit{hidden === 1 ? '' : 's'} of dependency updates by a bot not shown</span
          >{/if}
      </div>
      <ul>
        {#each found as r (r.sha)}
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
      {#if range}
        <button class="w-full px-1 text-left hover:bg-[var(--line)]" data-commit-range onclick={() => onrange?.(range.base, range.head)}>
          open range <code>{range.base}..{range.head}</code>
          <span class="text-[var(--muted)]">· the tree of the second against the tree of the first (Enter)</span>
        </button>
      {:else if other}
        <button class="w-full px-1 text-left hover:bg-[var(--line)]" data-commit-sha={other} onclick={() => onsha(other)}>
          open commit <code>{other}</code>
          <span class="text-[var(--muted)]">· not among the {rows.length} commits listed; {asks} (Enter)</span>
        </button>
      {:else if found.length === 0 && rows.length > 0}
        <div class="text-[var(--muted)]" data-no-commit>no match among the {rows.length} commits listed</div>
      {/if}
      {#if more}
        <button class="mt-1 border border-[var(--line)] px-2 py-0.5 hover:border-[var(--muted)]" data-more-commits onclick={onmore}>load more commits</button>
      {/if}
    {/if}
  </div>
</div>
