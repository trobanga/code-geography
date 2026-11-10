<script lang="ts">
  import type { Source } from '../lib/hash';
  import OpenButtons from './OpenButtons.svelte';
  import type { Collect } from '../lib/walk';
  import { KIND_LETTER, kindColor } from '../lib/kinds';
  import { LEVELS, type Level, type NodeKind } from '../lib/types';

  let {
    src,
    hasCommit,
    onsrc,
    level,
    tests,
    busy,
    moved,
    onlevel,
    ontests,
    onfit,
    onreset,
    onfile,
    ondir,
  }: {
    /** null: a file opened from disk. */
    src: Source | null;
    hasCommit: boolean;
    onsrc: (s: Source) => void;
    level: Level | null;
    tests: boolean;
    busy: boolean;
    moved: boolean;
    onlevel: (l: Level) => void;
    ontests: (t: boolean) => void;
    onfit: () => void;
    onreset: () => void;
    onfile: (f: File) => void;
    ondir: (collect: Collect) => void;
  } = $props();

  const kinds: NodeKind[] = ['module', 'package', 'type', 'function'];
  const btn = 'px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]';
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 whitespace-nowrap border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1.5">
  <div class="flex">
    {#each ['graph', 'commit'] as const as s (s)}
      <button
        class="{btn} -ml-px first:ml-0 disabled:opacity-40 {src === s ? 'bg-[var(--fg)] text-[var(--bg)]' : ''}"
        disabled={s === 'commit' && !hasCommit}
        onclick={() => onsrc(s)}>{s}</button
      >
    {/each}
  </div>
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
  <OpenButtons {btn} {onfile} {ondir} />
  <span class="text-[var(--muted)]">{busy ? 'layout…' : ''}</span>
  <div class="ml-auto flex items-center gap-3 text-[var(--muted)]">
    {#each kinds as k (k)}
      <span class="flex items-center gap-1">
        <span class="font-bold" style="color: {kindColor(k)}">{KIND_LETTER[k]}</span>{k}
      </span>
    {/each}
  </div>
</div>
