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
    partial,
    depth,
    tests,
    busy,
    moved,
    onlevel,
    ontests,
    onfit,
    onreset,
    onfile,
    ondir,
    ongithub,
  }: {
    /** null: a file opened from disk. */
    src: Source | null;
    hasCommit: boolean;
    onsrc: (s: Source) => void;
    level: Level | null;
    /** Between two levels: the deeper one whose containers are all open; extra containers are open too. */
    partial: { level: Level; extra: number } | null;
    depth: {
      canDeeper: boolean;
      canShallower: boolean;
      /** "+" waits for a second click: the number of boxes it gives. */
      ask: number | null;
      ondeeper: () => void;
      onshallower: () => void;
    };
    tests: boolean;
    busy: boolean;
    moved: boolean;
    onlevel: (l: Level) => void;
    ontests: (t: boolean) => void;
    onfit: () => void;
    onreset: () => void;
    onfile: (f: File) => void;
    ondir: (collect: Collect) => void;
    ongithub: (input: string) => void;
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
        class="{btn} -ml-px first:ml-0 {level === l
          ? 'bg-[var(--fg)] text-[var(--bg)]'
          : partial?.level === l
            ? 'bg-[var(--line)]'
            : ''}"
        title={partial?.level === l ? `${l} and ${partial.extra} more open containers` : undefined}
        onclick={() => onlevel(l)}>{l}</button
      >
    {/each}
  </div>
  <div class="flex items-center">
    <button
      class="{btn} disabled:opacity-40"
      title="one layer less (-)"
      disabled={!depth.canShallower}
      onclick={depth.onshallower}>−</button
    >
    <button
      class="{btn} -ml-px disabled:opacity-40 {depth.ask !== null ? 'border-[var(--accent)] text-[var(--accent)]' : ''}"
      title={depth.ask !== null ? `click again to show ${depth.ask} boxes` : 'one layer more (+)'}
      disabled={!depth.canDeeper}
      onclick={depth.ondeeper}>{depth.ask !== null ? `+ ${depth.ask.toLocaleString('en')} boxes?` : '+'}</button
    >
  </div>
  <label class="flex items-center gap-1">
    <input type="checkbox" checked={tests} onchange={(e) => ontests(e.currentTarget.checked)} />
    test code
  </label>
  <button class={btn} onclick={onfit}>fit</button>
  <button class="{btn} disabled:opacity-40" disabled={!moved} onclick={onreset}>reset positions</button>
  <OpenButtons {btn} {onfile} {ondir} {ongithub} />
  <span class="text-[var(--muted)]">{busy ? 'layout…' : ''}</span>
  <div class="ml-auto flex items-center gap-3 text-[var(--muted)]">
    {#each kinds as k (k)}
      <span class="flex items-center gap-1">
        <span class="font-bold" style="color: {kindColor(k)}">{KIND_LETTER[k]}</span>{k}
      </span>
    {/each}
  </div>
</div>
