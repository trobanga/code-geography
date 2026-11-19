<script lang="ts">
  import { findBoxes, type BoxEntry } from '../lib/boxSearch';
  import { KIND_LETTER, kindColor } from '../lib/kinds';

  let {
    index,
    tests,
    onpick,
  }: {
    index: BoxEntry[];
    tests: boolean;
    onpick: (id: string) => void;
  } = $props();

  let query = $state('');
  let focused = $state(false);
  let active = $state(0);
  let field: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  const found = $derived(findBoxes(index, query, tests));
  const open = $derived(focused && query.trim() !== '');

  // The active row stays in view when the arrow keys move it below the edge of the list.
  $effect(() => {
    list?.querySelector(`[data-row="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  });

  const pick = (id: string) => {
    onpick(id);
    query = '';
    active = 0;
  };

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = Math.max(0, Math.min(active + 1, found.matches.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      const m = found.matches[active];
      if (m) pick(m.id);
    } else if (e.key === 'Escape') {
      query = '';
      field?.blur();
    }
  }
</script>

<div class="relative tabular-nums" data-box-search>
  <input
    bind:this={field}
    bind:value={query}
    class="w-40 border border-[var(--line)] bg-transparent px-1 py-0.5 focus:border-[var(--muted)] focus:outline-none"
    type="text"
    spellcheck="false"
    autocomplete="off"
    placeholder="find a box"
    aria-label="find a box by name"
    role="combobox"
    aria-autocomplete="list"
    aria-expanded={open}
    aria-controls="box-list"
    aria-activedescendant={open && found.matches[active] ? `box-opt-${active}` : undefined}
    data-box-query
    onfocus={() => (focused = true)}
    onblur={() => (focused = false)}
    oninput={() => (active = 0)}
    {onkeydown}
  />
  {#if open}
    <div
      bind:this={list}
      class="absolute left-0 top-full z-30 mt-px max-h-80 w-[32rem] max-w-[90vw] overflow-y-auto border border-[var(--line)] bg-[var(--panel)] px-1 py-1"
      data-box-list
    >
      {#if found.matches.length === 0}
        <div class="text-[var(--muted)]" data-box-none>no box with that name</div>
      {:else}
        <!-- Focus stays in the field (mousedown keeps it there); the field names the active option. -->
        <ul id="box-list" role="listbox" aria-label="matching boxes">
          {#each found.matches as m, i (m.id)}
            <li
              id="box-opt-{i}"
              class="flex w-full cursor-pointer items-baseline gap-2 px-1 hover:bg-[var(--line)] {i === active ? 'bg-[var(--line)]' : ''}"
              role="option"
              aria-selected={i === active}
              data-box={m.id}
              data-row={i}
              title={m.path === '' ? m.name : `${m.path} › ${m.name}`}
              onmousedown={(e) => {
                e.preventDefault();
                pick(m.id);
              }}
            >
              <span class="w-3 shrink-0 font-bold" style="color: {kindColor(m.kind)}">{KIND_LETTER[m.kind]}</span>
              <span class="shrink-0">{m.name}</span>
              <span class="min-w-0 flex-1 truncate text-[var(--muted)]">{m.path}</span>
              {#if m.hidden}
                <span class="shrink-0 border border-[var(--line)] px-1 text-[var(--muted)]" data-box-test>test</span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if found.total > found.matches.length}
          <div class="px-1 text-[var(--muted)]" data-box-more>{found.total - found.matches.length} more, type more of the name</div>
        {/if}
      {/if}
    </div>
  {/if}
</div>
