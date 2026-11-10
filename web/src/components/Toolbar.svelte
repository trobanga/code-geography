<script lang="ts">
  import { KIND_LETTER, kindColor } from '../lib/kinds';
  import { LEVELS, type Level, type NodeKind } from '../lib/types';

  let {
    level,
    tests,
    busy,
    moved,
    onlevel,
    ontests,
    onfit,
    onreset,
    onfile,
  }: {
    level: Level | null;
    tests: boolean;
    busy: boolean;
    moved: boolean;
    onlevel: (l: Level) => void;
    ontests: (t: boolean) => void;
    onfit: () => void;
    onreset: () => void;
    onfile: (f: File) => void;
  } = $props();

  const kinds: NodeKind[] = ['module', 'package', 'type', 'function'];
  const btn = 'px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]';
</script>

<div class="flex items-center gap-3 border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1.5">
  <div class="flex">
    {#each LEVELS as l (l)}
      <button
        class="{btn} -ml-px first:ml-0 {level === l ? 'bg-[var(--fg)] text-[var(--bg)]' : ''}"
        onclick={() => onlevel(l)}>{l}</button
      >
    {/each}
  </div>
  <label class="flex items-center gap-1">
    <input type="checkbox" checked={tests} onchange={(e) => ontests(e.currentTarget.checked)} />
    test code
  </label>
  <button class={btn} onclick={onfit}>fit</button>
  <button class="{btn} disabled:opacity-40" disabled={!moved} onclick={onreset}>reset positions</button>
  <label class="{btn} cursor-pointer">
    open file
    <input
      type="file"
      accept=".json,application/json"
      class="hidden"
      onchange={(e) => {
        const f = e.currentTarget.files?.[0];
        if (f) onfile(f);
        e.currentTarget.value = '';
      }}
    />
  </label>
  <span class="text-[var(--muted)]">{busy ? 'layout…' : ''}</span>
  <div class="ml-auto flex items-center gap-3 text-[var(--muted)]">
    {#each kinds as k (k)}
      <span class="flex items-center gap-1">
        <span class="font-bold" style="color: {kindColor(k)}">{KIND_LETTER[k]}</span>{k}
      </span>
    {/each}
  </div>
</div>
