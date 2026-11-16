<script lang="ts">
  import { commitContext, commitView, type ContextCache, type ContextState } from '../lib/commitContext';
  import type { CommitSource, Repo } from '../lib/forge';
  import type { Range } from '../lib/forgeDiff';

  let { source, repo, range, cache }: { source: CommitSource; repo: Repo; range: Range; cache: ContextCache } = $props();

  let st = $state.raw<ContextState>({ open: false, commits: null, total: 0, status: null, current: null, pulls: null, pullStatus: null });
  // One context per strip; App.svelte keys the strip by the commit, so the props are read once.
  // svelte-ignore state_referenced_locally
  const ctx = commitContext({ source, repo, range, cache, onState: (s) => (st = s) });

  const view = $derived.by(() => {
    const c = st.commits?.find((x) => x.sha === st.current);
    return c ? commitView(repo, c, st.pulls ?? []) : null;
  });
  const failed = (line: string) => line.startsWith('cannot');
  const link = 'underline decoration-[var(--line)] hover:decoration-[var(--muted)]';
</script>

<div class="border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1">
  <button
    type="button"
    class="px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]"
    aria-expanded={st.open}
    onclick={() => ctx.toggle()}
  >
    message {st.open ? '▾' : '▸'}
  </button>
  {#if st.open}
    <div class="mt-1 max-h-[40vh] overflow-auto">
      {#if st.status}
        <div class={failed(st.status) ? 'text-[var(--accent)]' : 'text-[var(--muted)]'}>{st.status}</div>
      {/if}
      {#if st.commits && st.commits.length > 1}
        <ul>
          {#each st.commits as c (c.sha)}
            <li>
              <button type="button" class="w-full text-left hover:bg-[var(--line)] {c.sha === st.current ? 'font-semibold' : ''}" onclick={() => ctx.openCommit(c.sha)}>
                <span class="tabular-nums text-[var(--muted)]">{c.sha.slice(0, 7)}</span>
                {c.title}
              </button>
            </li>
          {/each}
        </ul>
        {#if st.total > st.commits.length}
          <div class="text-[var(--muted)]">{st.commits.length} of {st.total} commits listed</div>
        {/if}
      {/if}
      {#if view}
        <div class="mt-1">
          <a class="tabular-nums {link}" href={view.url} target="_blank" rel="noopener noreferrer">{view.short}</a>
          <span class="font-semibold">{view.title}</span>
        </div>
        {#if view.body}
          <pre class="whitespace-pre-wrap font-[inherit]">{view.body}</pre>
        {/if}
        {#if st.pullStatus}
          <div class={failed(st.pullStatus) ? 'text-[var(--accent)]' : 'text-[var(--muted)]'}>{st.pullStatus}</div>
        {/if}
        {#if view.pulls.length > 0}
          <div class="mt-1 text-[var(--muted)]">pull requests</div>
          <ul>
            {#each view.pulls as p (p.number)}
              <li>
                <a class={link} href={p.url} target="_blank" rel="noopener noreferrer">{p.label} {p.title}</a>
                <span class="text-[var(--muted)]">{p.state}</span>
              </li>
            {/each}
          </ul>
        {/if}
        {#if view.refs.length > 0}
          <div class="mt-1 text-[var(--muted)]">references</div>
          <ul>
            {#each view.refs as r (r.url)}
              <li><a class={link} href={r.url} target="_blank" rel="noopener noreferrer">{r.label}</a></li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>
  {/if}
</div>
