<script lang="ts">
  import type { Tree } from '../lib/expansion';
  import type { Focus } from '../lib/focus';

  let {
    tree,
    focus,
    rest,
    unfolded,
    onneighbors,
    onclear,
  }: {
    tree: Tree;
    focus: Focus;
    /** Folded children per container, from the view. */
    rest: Record<string, number> | undefined;
    /** Number of containers that show all children. */
    unfolded: number;
    onneighbors: (v: boolean) => void;
    onclear: () => void;
  } = $props();

  const btn = 'px-2 py-0.5 border border-[var(--line)] hover:border-[var(--muted)]';
  const what = $derived(
    [...(focus.changes ? ['changes'] : []), ...focus.nodes.map((id) => tree.label.get(id) ?? id)].join(', '),
  );
  const folded = $derived(Object.values(rest ?? {}).reduce((a, n) => a + n, 0));
  const boxes = $derived(Object.keys(rest ?? {}).length);
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--line)] bg-[var(--panel)] px-3 py-1 tabular-nums">
  <span class="text-[var(--muted)]">focus</span>
  <span class="truncate" title={focus.nodes.join(', ')}>{what}</span>
  <span class="text-[var(--muted)]"
    >{folded} folded in {boxes} {boxes === 1 ? 'box' : 'boxes'}{unfolded > 0
      ? `, ${unfolded} unfolded`
      : ''}</span
  >
  <label class="flex items-center gap-1 border-l border-[var(--line)] pl-3">
    <input type="checkbox" checked={focus.neighbors} onchange={(e) => onneighbors(e.currentTarget.checked)} />neighbors
  </label>
  <button class={btn} onclick={onclear}>show all</button>
</div>
