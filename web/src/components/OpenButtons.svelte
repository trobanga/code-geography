<script lang="ts">
  import { collectDir, collectFiles, fromHandle, isCancel, type Collect, type DirHandle } from '../lib/walk';
  import { parseSource } from '../lib/forges';
  import type { TokenField } from '../lib/tokenStore';

  let {
    onfile,
    ondir,
    onrepo,
    auth,
    btn,
  }: {
    onfile: (f: File) => void;
    /** A chosen directory, by the picker or by the input. */
    ondir: (collect: Collect) => void;
    /** The text of the repository field, as typed (ADR 0020, 0022). */
    onrepo: (input: string) => void;
    auth: TokenField;
    btn: string;
  } = $props();
  let repoText = $state('');
  // T-4: bound to a password field only; emptied once handed over, never shown as text.
  let tokenText = $state('');
  let keep = $state(false);
  /** The host that a token would be for: the one typed, else the one shown or asked for by the hash. GitHub: none. */
  const origin = $derived.by(() => {
    const p = repoText.trim() ? parseSource(repoText) : null;
    return p ? (p.ok ? (p.repo.origin ?? null) : null) : auth.origin;
  });
  const kept = $derived(origin && (auth.rev, auth.kept(origin)));
  const host = $derived(origin && new URL(origin).host);

  // Chromium browsers: the picker hands over a handle, and the walk lists only what can hold inputs.
  // Elsewhere the input lists every file below the directory before any code runs.
  const win = globalThis as { showDirectoryPicker?: (o?: { id?: string; mode?: 'read' }) => Promise<DirHandle> };
  const picker = typeof win.showDirectoryPicker === 'function';
  let dirInput: HTMLInputElement | undefined = $state();

  async function pick() {
    try {
      const h = await win.showDirectoryPicker!({ id: 'project', mode: 'read' });
      ondir(collectDir(fromHandle(h)));
    } catch (e) {
      if (!isCancel(e)) ondir(() => Promise.reject(e));
    }
  }
</script>

<button
  type="button"
  class="{btn} cursor-pointer"
  title={picker
    ? 'Index a Java or Rust project in this browser. Not stored: a reload returns to graph.json.'
    : 'Index a Java or Rust project in this browser. This browser lists every file of the directory first, which is slow for a tree with node_modules or target: drop the directory on the page instead.'}
  onclick={() => (picker ? pick() : dirInput?.click())}
>
  open directory
</button>
{#if !picker}
  <span class="text-[var(--muted)]" data-drop-advice>or drop it (faster)</span>
{/if}
<input
  bind:this={dirInput}
  type="file"
  webkitdirectory
  multiple
  class="hidden"
  data-open="directory"
  onchange={(e) => {
    const fs = [...(e.currentTarget.files ?? [])];
    e.currentTarget.value = '';
    if (fs.length > 0) ondir(collectFiles(fs));
  }}
/>
<label class="{btn} cursor-pointer" title="Open a graph.json made by cg index">
  open file
  <input
    type="file"
    accept=".json,application/json"
    class="hidden"
    data-open="file"
    onchange={(e) => {
      const f = e.currentTarget.files?.[0];
      if (f) onfile(f);
      e.currentTarget.value = '';
    }}
  />
</label>
<form
  class="flex items-center gap-1"
  data-open="repository"
  onsubmit={(e) => {
    e.preventDefault();
    if (origin && tokenText) auth.save(origin, tokenText, keep);
    tokenText = '';
    if (repoText.trim()) onrepo(repoText);
    else if (origin) auth.retry();
  }}
>
  <input
    type="text"
    bind:value={repoText}
    placeholder="owner/repo or URL"
    aria-label="repository"
    title="A public GitHub repository: owner/repo, owner/repo@ref, or a github.com URL. A repository of a Forgejo host (also private, with a token): its URL, or host/owner/repo@ref. With @A..B, @A...B, or a URL of a commit, a compare or a pull request: the commit overlay. The files are fetched by this browser and indexed here."
    class="w-44 border border-[var(--line)] bg-transparent px-1 py-0.5"
    spellcheck="false"
  />
  {#if origin}
    {#if kept}
      <span class="text-[var(--muted)]" data-token-kept>token for {host} ({kept === 'local' ? 'this device' : kept === 'session' ? 'this tab' : 'this page'})</span>
      <button
        type="button"
        class="{btn} cursor-pointer"
        title="Removes the token of {host} from this browser, and all cached files and trees of every host (a cached file does not know its host)"
        onclick={() => auth.forget(origin)}>forget token</button
      >
    {:else}
      <input
        type="password"
        autocomplete="off"
        bind:value={tokenText}
        placeholder="token for {host}"
        aria-label="access token for {host}"
        title="For a private repository: a Forgejo access token (Settings → Applications) with the scope read:repository only. Sent only to {origin}; kept for this tab unless you tick the box."
        class="w-36 border border-[var(--line)] bg-transparent px-1 py-0.5"
        spellcheck="false"
        data-token
      />
      <label class="flex items-center gap-1" title="Keep the token in the local storage of this browser after the tab closes"
        ><input type="checkbox" bind:checked={keep} />keep on this device</label
      >
      <span class="text-[var(--muted)]">scope read:repository</span>
    {/if}
  {/if}
  <button type="submit" class="{btn} cursor-pointer">open repository</button>
</form>
