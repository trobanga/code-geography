<script lang="ts">
  import { badgeLabel, changeLabel } from '../lib/commit';
  import { breakdown } from '../lib/detail';
  import { borderLine, edgePoints, placeNodes, type Offset, type Rect } from '../lib/drag';
  import { changed, painted, type Drawn } from '../lib/edgeOptions';
  import { BOX } from '../lib/elkGraph';
  import type { Tree } from '../lib/expansion';
  import { CHANGE_SIGN, changeColor, KIND_LETTER, kindColor } from '../lib/kinds';
  import type { LaidNode, Layout, Point } from '../lib/layout';
  import type { View, ViewEdge } from '../lib/types';
  import { fitTransform, labelAnchor, zoomAt, type Transform } from '../lib/viewport';

  let {
    tree,
    view,
    layout,
    drawn,
    onSelection,
    commit,
    changesOnly,
    offsets,
    expanded,
    selected,
    hoverNode = $bindable(null),
    onselect,
    ontoggle,
    onoffset,
  }: {
    tree: Tree;
    view: View;
    layout: Layout;
    /** The edge list `layout` was computed for; LaidEdge.index points into drawn.edges. */
    drawn: Drawn;
    onSelection: boolean;
    /** A commit graph is loaded: draw the overlay. */
    commit: boolean;
    /** Paint only changed edges and draw untouched boxes pale. */
    changesOnly: boolean;
    offsets: ReadonlyMap<string, Offset>;
    expanded: ReadonlySet<string>;
    selected: string | null;
    hoverNode?: string | null;
    onselect: (id: string | null) => void;
    ontoggle: (id: string) => void;
    onoffset: (id: string, o: Offset) => void;
  } = $props();

  let wrap: HTMLDivElement;
  let vw = $state(0);
  let vh = $state(0);
  let t = $state<Transform>({ x: 0, y: 0, k: 1 });
  let hover = $state<{ edge: ViewEdge; x: number; y: number } | null>(null);

  export function fit() {
    if (vw > 0 && vh > 0) t = fitTransform(layout, { width: vw, height: vh }, 16);
  }

  const expandable = $derived(new Set(view.expandable));
  const placed = $derived(placeNodes(layout.nodes, offsets));
  const rects = $derived(new Map<string, Rect>(placed.nodes.map((n) => [n.id, n])));
  const containers = $derived(placed.nodes.filter((n) => n.container).sort((a, b) => a.depth - b.depth));
  const leaves = $derived(placed.nodes.filter((n) => !n.container));
  const ZERO = { dx: 0, dy: 0 };
  const width = (count: number) => Math.min(6, 0.75 + 0.6 * Math.log2(count));
  const pathOf = (pts: Point[]) => 'M' + pts.map((p) => `${p.x},${p.y}`).join('L');
  const edges = $derived(
    layout.edges
      .map((le) => {
        const ve = drawn.edges[le.index];
        const touches = selected !== null && (ve.from === selected || ve.to === selected);
        const a = rects.get(ve.from);
        const b = rects.get(ve.to);
        const pts =
          a && b
            ? edgePoints(le.points, placed.shift.get(ve.from) ?? ZERO, placed.shift.get(ve.to) ?? ZERO, a, b)
            : le.points;
        return {
          key: le.index,
          ve,
          touches,
          show: painted(ve, onSelection, selected, hoverNode, changesOnly),
          d: pathOf(pts),
          w: width(ve.count),
          lab: labelAnchor(pts),
          ch: edgeChange(ve),
        };
      })
      .filter((e) => e.show),
  );
  // O3: the real edges of the selected node that were lifted away, as direct lines.
  const realLines = $derived(
    selected === null
      ? []
      : drawn.lifted
          .filter((e) => e.from === selected || e.to === selected)
          .flatMap((ve) => {
            const a = rects.get(ve.from);
            const b = rects.get(ve.to);
            if (!a || !b) return [];
            const pts = borderLine(a, b);
            return [{ ve, d: pathOf(pts), w: width(ve.count), lab: labelAnchor(pts), ch: edgeChange(ve) }];
          }),
  );
  const nameOf = (id: string) => tree.label.get(id) ?? id;

  /** Overlay look of an edge: color by the larger of added and removed, dashed when all removed. */
  function edgeChange(ve: ViewEdge) {
    if (!commit || !changed(ve)) return null;
    const kind = ve.added >= ve.removed ? 'added' : 'removed';
    return { color: `var(--c-${kind})`, marker: `url(#arr-${kind})`, dashed: ve.removed === ve.count, label: changeLabel(ve) };
  }
  const SEGMENTS = ['added', 'removed', 'modified'] as const;

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

  /** Movement in screen pixels below which a press is still a click. */
  const THRESHOLD = 3;
  type Drag =
    | { kind: 'pan'; x: number; y: number; tx: number; ty: number; moved: boolean }
    | { kind: 'node'; id: string; x: number; y: number; start: Offset; moved: boolean };
  let drag: Drag | null = null;
  /** Set when a node drag ends; swallows the click that the browser sends after it. */
  let swallowClick = false;
  let lastNodeDrag = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    swallowClick = false;
    const node = (e.target as Element).closest('[data-node]')?.getAttribute('data-node');
    if (node) {
      drag = { kind: 'node', id: node, x: e.clientX, y: e.clientY, start: offsets.get(node) ?? ZERO, moved: false };
      return;
    }
    drag = { kind: 'pan', x: e.clientX, y: e.clientY, tx: t.x, ty: t.y, moved: false };
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (!drag.moved && Math.abs(dx) + Math.abs(dy) <= THRESHOLD) return;
    if (!drag.moved) {
      drag.moved = true;
      if (drag.kind === 'node') (e.currentTarget as Element).setPointerCapture(e.pointerId);
    }
    if (drag.kind === 'pan') t = { ...t, x: drag.tx + dx, y: drag.ty + dy };
    else onoffset(drag.id, { dx: drag.start.dx + dx / t.k, dy: drag.start.dy + dy / t.k });
  }
  function up() {
    if (drag?.kind === 'pan' && !drag.moved) onselect(null);
    if (drag?.kind === 'node' && drag.moved) {
      swallowClick = true;
      lastNodeDrag = performance.now();
    }
    drag = null;
  }
  function clickCapture(e: MouseEvent) {
    if (!swallowClick) return;
    swallowClick = false;
    e.stopPropagation();
  }
  function dblclickCapture(e: MouseEvent) {
    if (performance.now() - lastNodeDrag < 600) e.stopPropagation();
  }
  function over(e: PointerEvent) {
    if (drag?.moved) return;
    const el = e.target as Element;
    if (el.closest('[data-edge]')) return;
    hoverNode = el.closest('[data-node]')?.getAttribute('data-node') ?? null;
  }
  function edgeHover(e: PointerEvent, edge: ViewEdge) {
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
    {@const mid = n.container ? n.y + BOX.header / 2 : n.y + n.h / 2}
    {@const own = commit ? g.change : undefined}
    {@const below = commit ? view.changes[n.id] : undefined}
    {@const label = tree.label.get(n.id) ?? n.id}
    <g
      data-node={n.id}
      opacity={changesOnly && !below ? 0.3 : 1}
      class="cursor-default outline-none"
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
        stroke={sel ? 'var(--accent)' : own ? changeColor(own) : c}
        stroke-width={sel ? 2 : own ? 1.25 : n.container ? 0.75 : 0.6}
        stroke-dasharray={own === 'removed' ? '4 2' : undefined}
      />
      {#if below && !own}
        <!-- Changes below: a bar at the left edge, split by kind of mark. -->
        {@const total = below.added + below.removed + below.modified}
        {@const h = n.container ? BOX.header : n.h}
        {#each SEGMENTS as k, i (k)}
          {@const y0 = SEGMENTS.slice(0, i).reduce((a, j) => a + below[j], 0)}
          {#if below[k] > 0}
            <rect x={n.x} y={n.y + (h * y0) / total} width="3" height={(h * below[k]) / total} fill={changeColor(k)} />
          {/if}
        {/each}
      {/if}
      <text x={n.x + BOX.pad} y={mid + 4} font-size={BOX.font} font-weight="700" fill={c}>{KIND_LETTER[g.kind]}</text>
      <text
        x={n.x + BOX.pad + BOX.kindW}
        y={mid + 4}
        font-size={BOX.font}
        fill="var(--fg)"
        text-decoration={own === 'removed' ? 'line-through' : undefined}>{label}</text
      >
      {#if own}
        <text
          x={n.x + BOX.pad + BOX.kindW + label.length * BOX.charW + 3}
          y={mid + 4}
          font-size={BOX.font}
          font-weight="700"
          fill={changeColor(own)}><title>{own}</title>{CHANGE_SIGN[own]}</text
        >
      {/if}
      {#if below && badgeLabel(below, own)}
        <text x={n.x} y={n.y - 2} font-size="8" fill="var(--muted)"
          ><title>marked below: {below.added} added, {below.removed} removed, {below.modified} modified{own
              ? ' (with this node)'
              : ''}</title
          >{badgeLabel(below, own)}</text
        >
      {/if}
      {@render marker(n, n.x + n.w - BOX.markerW + 1, mid - 5.5)}
      {#if drawn.badges.get(n.id)}
        {@const b = drawn.badges.get(n.id)!}
        <text
          x={n.x + n.w}
          y={n.y - 2}
          text-anchor="end"
          font-size="8"
          fill="var(--muted)"
          ><title>lifted away: {b.out} out, {b.in} in</title>{b.out ? `→${b.out}` : ''}{b.out && b.in ? ' ' : ''}{b.in
            ? `←${b.in}`
            : ''}</text
        >
      {/if}
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
    onpointerover={over}
    onpointerleave={() => (hoverNode = null)}
    onclickcapture={clickCapture}
    ondblclickcapture={dblclickCapture}
  >
    <defs>
      <marker id="arr" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
        <path d="M0,0L8,4L0,8z" fill="var(--edge)" />
      </marker>
      {#each ['added', 'removed'] as k (k)}
        <marker id="arr-{k}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="9" markerHeight="9" markerUnits="userSpaceOnUse" orient="auto">
          <path d="M0,0L8,4L0,8z" fill="var(--c-{k})" />
        </marker>
      {/each}
      <marker id="arr-hi" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="9" markerHeight="9" markerUnits="userSpaceOnUse" orient="auto">
        <path d="M0,0L8,4L0,8z" fill="var(--accent)" />
      </marker>
    </defs>
    <g transform="translate({t.x},{t.y}) scale({t.k})">
      {#each containers as n (n.id)}{@render box(n)}{/each}
      {#each leaves as n (n.id)}{@render box(n)}{/each}
      {#each edges as e (e.key)}
        <g
          data-edge
          opacity={selected === null || e.touches || onSelection ? 1 : 0.15}
          role="presentation"
          onpointerenter={(ev) => edgeHover(ev, e.ve)}
          onpointermove={(ev) => edgeHover(ev, e.ve)}
          onpointerleave={() => (hover = null)}
        >
          <path d={e.d} fill="none" stroke="transparent" stroke-width={Math.max(8, e.w + 4)} />
          {#if !e.ch?.dashed}
            <path
              d={e.d}
              fill="none"
              stroke={e.touches ? 'var(--accent)' : 'var(--edge)'}
              stroke-width={e.w}
              stroke-opacity={commit && !e.touches ? 0.35 : 0.8}
              marker-end={e.ch ? undefined : e.touches ? 'url(#arr-hi)' : 'url(#arr)'}
            />
          {/if}
          {#if e.ch}
            <!-- The changed part on top of the whole bundle, as wide as the changed count. -->
            <path
              d={e.d}
              fill="none"
              stroke={e.ch.color}
              stroke-width={Math.max(1.5, width(e.ve.added + e.ve.removed))}
              stroke-dasharray={e.ch.dashed ? '5 3' : undefined}
              marker-end={e.ch.marker}
            />
          {/if}
        </g>
      {/each}
      {#each realLines as e (e.ve.from + '>' + e.ve.to)}
        <g
          data-edge
          role="presentation"
          onpointerenter={(ev) => edgeHover(ev, e.ve)}
          onpointermove={(ev) => edgeHover(ev, e.ve)}
          onpointerleave={() => (hover = null)}
        >
          <path d={e.d} fill="none" stroke="transparent" stroke-width={Math.max(8, e.w + 4)} />
          <path
            d={e.d}
            fill="none"
            stroke={e.ch ? e.ch.color : 'var(--accent)'}
            stroke-width={e.w}
            stroke-dasharray="5 3"
            marker-end={e.ch ? e.ch.marker : 'url(#arr-hi)'}
          />
        </g>
      {/each}
      {#each [...edges, ...realLines] as e, i (i)}
        {#if e.ch || e.ve.count > 1}
          {@const hi = !('touches' in e) || e.touches}
          <text
            x={e.lab.horizontal ? e.lab.x : e.lab.x + e.w / 2 + 2}
            y={e.lab.horizontal ? e.lab.y - e.w / 2 - 2 : e.lab.y + 3}
            text-anchor={e.lab.horizontal ? 'middle' : 'start'}
            font-size="9"
            fill={e.ch ? e.ch.color : hi ? 'var(--accent)' : 'var(--muted)'}
            font-weight={e.ch ? 700 : undefined}
            opacity={selected === null || hi || onSelection ? (commit && !e.ch && !hi ? 0.6 : 1) : 0.15}
            stroke="var(--bg)"
            stroke-width="3"
            paint-order="stroke"
            pointer-events="none">{e.ch ? e.ch.label : e.ve.count}</text
          >
        {/if}
      {/each}
    </g>
  </svg>
  {#if hover}
    {@const ve = hover.edge}
    <div
      class="pointer-events-none absolute border border-[var(--line)] bg-[var(--panel)] px-2 py-1 text-[11px] whitespace-nowrap"
      style="left: {hover.x + 12}px; top: {hover.y + 12}px"
    >
      <div>{nameOf(ve.from)} → {nameOf(ve.to)} <span class="text-[var(--muted)]">{ve.count}</span></div>
      <div class="text-[var(--muted)]">{breakdown(ve.kinds)}</div>
      {#if commit && changed(ve)}
        <div>
          <span style="color: var(--c-added)">added {ve.added}</span>
          <span style="color: var(--c-removed)">removed {ve.removed}</span>
          <span class="text-[var(--muted)]">unchanged {ve.count - ve.added - ve.removed}</span>
        </div>
      {/if}
    </div>
  {/if}
</div>
