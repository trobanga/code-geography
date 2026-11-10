<script lang="ts">
  let {
    onfile,
    ondir,
    btn,
  }: {
    onfile: (f: File) => void;
    /** All files of the chosen directory, each with its `webkitRelativePath`. */
    ondir: (files: File[]) => void;
    btn: string;
  } = $props();
</script>

<label class="{btn} cursor-pointer" title="Index a Java project in this browser. Not stored: a reload returns to graph.json.">
  open directory
  <input
    type="file"
    webkitdirectory
    multiple
    class="hidden"
    data-open="directory"
    onchange={(e) => {
      const fs = [...(e.currentTarget.files ?? [])];
      e.currentTarget.value = '';
      if (fs.length > 0) ondir(fs);
    }}
  />
</label>
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
