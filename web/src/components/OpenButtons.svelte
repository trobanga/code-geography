<script lang="ts">
  import { collectDir, collectFiles, fromHandle, isCancel, type Collect, type DirHandle } from '../lib/walk';

  let {
    onfile,
    ondir,
    ongithub,
    btn,
  }: {
    onfile: (f: File) => void;
    /** A chosen directory, by the picker or by the input. */
    ondir: (collect: Collect) => void;
    /** The text of the GitHub field, as typed (ADR 0020). */
    ongithub: (input: string) => void;
    btn: string;
  } = $props();
  let repoText = $state('');

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
  data-open="github"
  onsubmit={(e) => {
    e.preventDefault();
    if (repoText.trim()) ongithub(repoText);
  }}
>
  <input
    type="text"
    bind:value={repoText}
    placeholder="owner/repo[@ref]"
    aria-label="GitHub repository"
    title="A public GitHub repository: owner/repo, owner/repo@ref, or a github.com URL. With owner/repo@A..B, @A...B, or a URL /commit/<sha>, /compare/A...B or /pull/<n>: the commit overlay. The files are fetched by this browser and indexed here."
    class="w-44 border border-[var(--line)] bg-transparent px-1 py-0.5"
    spellcheck="false"
  />
  <button type="submit" class="{btn} cursor-pointer">open GitHub</button>
</form>
