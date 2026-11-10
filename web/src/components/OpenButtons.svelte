<script lang="ts">
  import { collectDir, collectFiles, fromHandle, isCancel, type Collect, type DirHandle } from '../lib/walk';

  let {
    onfile,
    ondir,
    btn,
  }: {
    onfile: (f: File) => void;
    /** A chosen directory, by the picker or by the input. */
    ondir: (collect: Collect) => void;
    btn: string;
  } = $props();

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
  title="Index a Java or Rust project in this browser. Not stored: a reload returns to graph.json."
  onclick={() => (picker ? pick() : dirInput?.click())}
>
  open directory
</button>
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
