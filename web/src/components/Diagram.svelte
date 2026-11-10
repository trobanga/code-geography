<script lang="ts">
  import { breakdown } from '../lib/detail';
  import { BOX } from '../lib/elkGraph';
  import type { Tree } from '../lib/expansion';
  import { KIND_LETTER, kindColor } from '../lib/kinds';
  import type { LaidEdge, LaidNode, Layout } from '../lib/layout';
  import type { View } from '../lib/types';
  import { fitTransform, polylineMidpoint, zoomAt, type Transform } from '../lib/viewport';

  let {
    tree,
    view,
    layout,
    expanded,
    selected,
    onselect,
    ontoggle,
  }: {
    tree: Tree;
    view: View;
    layout: Layout;
    expanded: ReadonlySet<string>;
    selected: string | null;
    onselect: (id: string | null) => void;
    ontoggle: (id: string) => void;
  } = $props();

  let wrap: HTMLDivElement;
  let vw = $state(0);
  let vh = $state(0);
  let t = $state<Transform>({ x: 0, y: 0, k: 1 });
  let hover = $state<{ edge: LaidEdge; x: number; y: number } | null>(null);

  export function fit() {
    if (vw > 0 && vh > 0) t = fitTransform(layout, { width: vw, height: vh }, 16);
  }

  const expandable = $derived(new Set(view.expandable));
  const containers = $derived(layout.nodes.filter((n) => n.container).sort((a, b) => a.depth - b.depth));
  const leaves = $derived(layout.nodes.filter((n) => !n.container));
  const edges = $derived(
    layout.edges.map((le) => {
      const ve = view.edges[le.index];
      const touches = selected !== null && (ve.from === selected || ve.to === selected);
      return {
        le,
        ve,
        touches,
        d: 'M' + le.points.map((p) => `${p.x},${p.y}`).join('L'),
        w: Math.min(6, 0.75 + 0.6 * Math.log2(ve.count)),
        mid: polylineMidpoint(le.points),
      };
    }),
  );
  const nameOf = (id: string) => tree.label.get(id) ?? id;

  // Wheel zoom at the pointer; needs a non-passive listener to stop page scroll.
  $effect(() => {
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = wrap.getBoundingClientRect();
      t = zoomAt(t, e.clientX - r.left, e.clientY - r.top, Math.exp(-e.deltaY * 0.0015));
    };
    wrap.addEventListener('wheel', onWheel, { passive: false });
    return () => wrap.removeEventListener('wheel', onWheel);
  });

  let drag: { x: number; y: number; tx: number; ty: number; moved: boolean } | null = null;
  function down(e: PointerEvent) {
    if (e.button !== 0 || (e.target as Element).closest('[data-node]')) return;
    drag = { x: e.clientX, y: e.clientY, tx: t.x, ty: t.y, moved: false };
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    t = { ...t, x: drag.tx + dx, y: drag.ty + dy };
  }
  function up() {
    if (drag && !drag.moved) onselect(null);
    drag = null;
  }
  function edgeHover(e: PointerEvent, edge: LaidEdge) {
    const r = wrap.getBoundingClientRect();
    hover = { edge, x: e.clientX - r.left, y: e.clientY - r.top };
  }
</script>

{#snippet marker(n: LaidNode, x: number, y: number)}
  {#if expandable.has(n.id)}
    <g
      class="cursor-pointer"
      role="button"
      tabindex="-1"
      onclick={(e) => {
        e.stopPropagation();
        ontoggle(n.id);
      }}
      ondblclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
    >
      <rect {x} {y} width="11" height="11" fill="var(--bg)" stroke="var(--muted)" stroke-width="0.75" />
      <path
        d={expanded.has(n.id) ? `M${x + 3},${y + 5.5}h5` : `M${x + 3},${y + 5.5}h5M${x + 5.5},${y + 3}v5`}
        stroke="var(--fg)"
        stroke-width="1"
      />
    </g>
  {/if}
{/snippet}

{#snippet box(n: LaidNode)}
  {@const g = tree.byId.get(n.id)}
  {#if g}
    {@const c = kindColor(g.kind)}
    {@const sel = n.id === selected}
    <g
      data-node
      class="cursor-default"
      role="button"
      tabindex="-1"
      onclick={() => onselect(n.id)}
      ondblclick={() => ontoggle(n.id)}
      onkeydown={() => {}}
    >
      <rect
        x={n.x}
        y={n.y}
        width={n.w}
        height={n.h}
        fill={n.container ? `color-mix(in srgb, ${c} 5%, var(--bg))` : `color-mix(in srgb, ${c} 12%, var(--bg))`}
        stroke={sel ? 'var(--accent)' : c}
        stroke-width={sel ? 2 : n.container ? 0.75 : 0.6}
      />
      <text x={n.x + BOX.pad} y={n.y + 15} font-size={BOX.font} font-weight="700" fill={c}>{KIND_LETTER[g.kind]}</text>
      <text x={n.x + BOX.pad + BOX.kindW} y={n.y + 15} font-size={BOX.font} fill="var(--fg)">{tree.label.get(n.id)}</text>
      {@render marker(n, n.x + n.w - BOX.markerW + 1, n.y + 5.5)}
    </g>
  {/if}
{/snippet}

<div
  bind:this={wrap}
  bind:clientWidth={vw}
  bind:clientHeight={vh}
  class="relative min-w-0 flex-1 overflow-hidden select-none"
>
  <svg
    class="block h-full w-full"
    role="presentation"
    font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
  >
    <defs>
      <marker id="arr" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
        <path d="M0,0L8,4L0,8z" fill="var(--edge)" />
      </marker>
      <marker id="arr-hi" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="9" markerHeight="9" markerUnits="userSpaceOnUse" orient="auto">
        <path d="M0,0L8,4L0,8z" fill="var(--accent)" />
      </marker>
    </defs>
    <g transform="translate({t.x},{t.y}) scale({t.k})">
      {#each containers as n (n.id)}{@render box(n)}{/each}
      {#each leaves as n (n.id)}{@render box(n)}{/each}
      {#each edges as e (e.le.index)}
        <g
          opacity={selected === null || e.touches ? 1 : 0.15}
          role="presentation"
          onpointerenter={(ev) => edgeHover(ev, e.le)}
          onpointermove={(ev) => edgeHover(ev, e.le)}
          onpointerleave={() => (hover = null)}
        >
          <path d={e.d} fill="none" stroke="transparent" stroke-width={Math.max(8, e.w + 4)} />
          <path
            d={e.d}
            fill="none"
            stroke={e.touches ? 'var(--accent)' : 'var(--edge)'}
            stroke-width={e.w}
            stroke-opacity="0.8"
            marker-end={e.touches ? 'url(#arr-hi)' : 'url(#arr)'}
          />
        </g>
      {/each}
      {#each edges as e (e.le.index)}
        {#if e.ve.count > 1}
          <text
            x={e.mid.x + e.w / 2 + 2}
            y={e.mid.y - 2}
            font-size="9"
            fill={e.touches ? 'var(--accent)' : 'var(--muted)'}
            opacity={selected === null || e.touches ? 1 : 0.15}
            stroke="var(--bg)"
            stroke-width="3"
            paint-order="stroke"
            pointer-events="none">{e.ve.count}</text
          >
        {/if}
      {/each}
    </g>
  </svg>
  {#if hover}
    {@const ve = view.edges[hover.edge.index]}
    <div
      class="pointer-events-none absolute border border-[var(--line)] bg-[var(--panel)] px-2 py-1 text-[11px] whitespace-nowrap"
      style="left: {hover.x + 12}px; top: {hover.y + 12}px"
    >
      <div>{nameOf(ve.from)} → {nameOf(ve.to)} <span class="text-[var(--muted)]">{ve.count}</span></div>
      <div class="text-[var(--muted)]">{breakdown(ve.kinds)}</div>
    </div>
  {/if}
</div>
