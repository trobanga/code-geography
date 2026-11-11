import type { ElkExtendedEdge, ElkNode } from 'elkjs/lib/elk-api';
import { drawable, parentOf, type Tree } from './expansion';
import { labelOf } from './focus';
import type { View } from './types';

/** Geometry shared by the ELK input and the SVG renderer. */
export const BOX = {
  font: 11,
  charW: 6.7,
  height: 22,
  pad: 6,
  kindW: 14,
  markerW: 14,
  /** Room for the mark of a changed node (`+`, `−`, `~`) after the name. */
  markW: 12,
  header: 22,
} as const;

export function labelWidth(name: string, expandable: boolean): number {
  return Math.ceil(BOX.pad + BOX.kindW + name.length * BOX.charW + BOX.pad + (expandable ? BOX.markerW : 0));
}

/** 1 for a view of at most SPARSE nodes, 0 from DENSE nodes on, linear between. */
const SPARSE = 6;
const DENSE = 60;
/** Views with more nodes share edge segments (fewer, thicker bundles). */
const MERGE_FROM = 20;
function sparseness(nodeCount: number): number {
  return Math.min(1, Math.max(0, (DENSE - nodeCount) / (DENSE - SPARSE)));
}
/** Height per edge on one side of a leaf in a sparse view. */
const EDGE_ROOM = 18;
const lerp = (dense: number, sparse: number, f: number) => String(Math.round(dense + (sparse - dense) * f));

/** ELK root options; spacing grows when the view has few nodes. */
export function rootOptions(nodeCount: number): Record<string, string> {
  const f = sparseness(nodeCount);
  return {
    'elk.algorithm': 'layered',
    'elk.direction': 'RIGHT',
    'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
    'elk.edgeRouting': 'ORTHOGONAL',
    'elk.spacing.nodeNode': lerp(14, 50, f),
    'elk.layered.spacing.nodeNodeBetweenLayers': lerp(28, 120, f),
    'elk.spacing.edgeNode': lerp(10, 24, f),
    'elk.spacing.edgeEdge': lerp(6, 16, f),
    'elk.layered.mergeEdges': String(nodeCount > MERGE_FROM),
    'elk.json.edgeCoords': 'ROOT',
    'elk.json.shapeCoords': 'ROOT',
  };
}

function containerPadding(nodeCount: number) {
  const f = sparseness(nodeCount);
  return { top: BOX.header + Number(lerp(6, 14, f)), side: Number(lerp(8, 24, f)) };
}

/** Edge ids are `e<index into view.edges>`. */
export function edgeIndex(elkEdgeId: string): number {
  return Number(elkEdgeId.slice(1));
}

/**
 * With INCLUDE_CHILDREN, ELK lays out the whole hierarchy as one graph in the root direction, so
 * nested chains add up to a wide strip, and a direction set on a container is ignored. When every
 * edge joins two siblings (as after lifting, O3), no edge crosses a container border, and each
 * container is laid out on its own, in a direction that alternates with depth unless the other one
 * gives a box much nearer a square (`layOut`).
 */
function containerDirection(depth: number): string {
  return depth % 2 === 0 ? 'DOWN' : 'RIGHT';
}

/** Layer of each node by the longest path from a source; edges that close a cycle are skipped. */
function longestPathLayers(ids: string[], out: Map<string, string[]>): Map<string, number> {
  const state = new Map<string, 'open' | 'done'>();
  const order: string[] = [];
  const visit = (v: string) => {
    state.set(v, 'open');
    for (const w of out.get(v) ?? []) if (!state.has(w)) visit(w);
    state.set(v, 'done');
    order.push(v);
  };
  for (const v of ids) if (!state.has(v)) visit(v);
  order.reverse();
  const rank = new Map(order.map((v, i) => [v, i]));
  const layer = new Map<string, number>(ids.map((v) => [v, 0]));
  for (const v of order)
    for (const w of out.get(v) ?? []) if (rank.get(w)! > rank.get(v)!) layer.set(w, Math.max(layer.get(w)!, layer.get(v)! + 1));
  return layer;
}

interface Size {
  w: number;
  h: number;
}
interface Spacing {
  node: number;
  layer: number;
}
/** Target ratio of ELK's packing of connected components (its default `elk.aspectRatio`). */
const PACK_RATIO = 1.6;

/**
 * Estimated box of siblings that the layered algorithm lays out in direction `dir`. Each connected
 * component is cut into layers by the longest path; in direction DOWN a layer is a row (its width
 * is the sum of the widths), in direction RIGHT a column. ELK packs the components side by side
 * near PACK_RATIO.
 */
function estimate(kids: string[], inner: { from: string; to: string }[], size: (id: string) => Size, sp: Spacing, dir: string): Size {
  const root = new Map(kids.map((k) => [k, k]));
  const find = (v: string): string => (root.get(v) === v ? v : find(root.get(v)!));
  for (const e of inner) root.set(find(e.from), find(e.to));
  const comps = new Map<string, string[]>();
  for (const k of kids) comps.set(find(k), [...(comps.get(find(k)) ?? []), k]);
  const out = new Map<string, string[]>();
  for (const e of inner) out.set(e.from, [...(out.get(e.from) ?? []), e.to]);
  const down = dir === 'DOWN';
  const boxes = [...comps.values()].map((comp) => {
    const layers = new Map<number, Size>();
    for (const [id, l] of longestPathLayers(comp, out)) {
      const s = size(id);
      const a = layers.get(l);
      // Along the layer: sizes add up; across it: the largest counts.
      layers.set(
        l,
        !a
          ? s
          : down
            ? { w: a.w + sp.node + s.w, h: Math.max(a.h, s.h) }
            : { w: Math.max(a.w, s.w), h: a.h + sp.node + s.h },
      );
    }
    const ls = [...layers.values()];
    const gaps = sp.layer * (ls.length - 1);
    return down
      ? { w: Math.max(...ls.map((l) => l.w)), h: ls.reduce((a, l) => a + l.h, gaps) }
      : { w: ls.reduce((a, l) => a + l.w, gaps), h: Math.max(...ls.map((l) => l.h)) };
  });
  if (boxes.length === 1) return boxes[0];
  const area = boxes.reduce((a, b) => a + (b.w + sp.node) * (b.h + sp.node), 0);
  const w = Math.max(...boxes.map((b) => b.w), Math.sqrt(area * PACK_RATIO));
  return { w, h: Math.max(...boxes.map((b) => b.h), area / w) };
}

/**
 * The estimate is rough (no edge routing, no crossing minimisation, packing guessed), so the other
 * direction must bring the box nearer a square by more than this factor of width:height. Of 1, 1.5
 * and 2, 2 changed the fewest containers of the plain views of the two test repositories and fixed
 * the commit strip just as well.
 */
const FLIP_GAIN = 2;

/**
 * The direction of a container and the estimated box of its children: `fallback` (by depth),
 * unless the other direction is nearer a square by FLIP_GAIN. Boxes are flat, so a wide layer (many
 * callers of one helper) goes RIGHT, a long chain goes DOWN.
 */
function layOut(kids: string[], edges: { from: string; to: string }[], size: (id: string) => Size, sp: Spacing, fallback: string) {
  const set = new Set(kids);
  const inner = edges.filter((e) => set.has(e.from) && set.has(e.to) && e.from !== e.to);
  const skew = (b: Size) => Math.abs(Math.log(b.w / b.h));
  const [first, second] = [fallback, fallback === 'DOWN' ? 'RIGHT' : 'DOWN'].map((d) => ({ dir: d, box: estimate(kids, inner, size, sp, d) }));
  return skew(second.box) < skew(first.box) - Math.log(FLIP_GAIN) ? second : first;
}

export function toElkGraph(view: View, tree: Tree, nodeCount = view.nodes.length, shown?: ReadonlySet<string>): ElkNode {
  const root: ElkNode = { id: 'root', layoutOptions: rootOptions(nodeCount), children: [], edges: [] };
  const visible = new Set(view.nodes);
  const kept = view.edges.filter((e) => visible.has(e.from) && visible.has(e.to) && drawable(tree, e));
  const separate = kept.every((e) => parentOf(tree, e.from) === parentOf(tree, e.to));
  if (separate) root.layoutOptions!['elk.hierarchyHandling'] = 'SEPARATE_CHILDREN';
  // Options of the layered algorithm for the children of one container: the routing of the root
  // and the dense spacing; the sparse spacing stays at the top level, where it was tuned.
  const { 'elk.json.edgeCoords': _e, 'elk.json.shapeCoords': _s, ...dense } = rootOptions(DENSE);
  const layered = {
    ...dense,
    'elk.hierarchyHandling': 'SEPARATE_CHILDREN',
    'elk.layered.mergeEdges': root.layoutOptions!['elk.layered.mergeEdges'],
  };
  const depthOf = (id: string) => {
    let d = 0;
    for (let p = parentOf(tree, id); p !== null; p = parentOf(tree, p)) d++;
    return d;
  };
  const expandable = new Set(view.expandable);
  const elkById = new Map<string, ElkNode>();
  const hasVisibleChild = new Set<string>();
  for (const id of view.nodes) {
    const p = parentOf(tree, id);
    if (p !== null && visible.has(p)) hasVisibleChild.add(p);
  }

  // In sparse views a leaf is tall enough to keep the edges on its busier side apart.
  const f = sparseness(nodeCount);
  const inDeg = new Map<string, number>();
  const outDeg = new Map<string, number>();
  for (const e of view.edges) {
    if (!visible.has(e.from) || !visible.has(e.to) || !drawable(tree, e)) continue;
    outDeg.set(e.from, (outDeg.get(e.from) ?? 0) + 1);
    inDeg.set(e.to, (inDeg.get(e.to) ?? 0) + 1);
  }
  const leafHeight = (id: string) => {
    const side = Math.max(inDeg.get(id) ?? 0, outDeg.get(id) ?? 0);
    if (nodeCount > MERGE_FROM) return BOX.height;
    return Math.round(BOX.height + f * Math.max(0, side * EDGE_ROOM - BOX.height));
  };

  const widthOf = (id: string) => labelWidth(labelOf(tree, view, id, shown), expandable.has(id)) + (tree.byId.get(id)?.change ? BOX.markW : 0);
  const spacing = { node: Number(dense['elk.spacing.nodeNode']), layer: Number(dense['elk.layered.spacing.nodeNodeBetweenLayers']) };
  const pad = containerPadding(nodeCount);
  // Bottom up: the direction of a container from the estimated sizes of its children.
  const laid = new Map<string, { dir: string; box: Size }>();
  const sizeOf = (id: string): Size => {
    if (!hasVisibleChild.has(id)) return { w: widthOf(id), h: leafHeight(id) };
    const l = layoutOf(id);
    return { w: Math.max(widthOf(id), l.box.w + 2 * pad.side), h: Math.max(BOX.header + 16, l.box.h + pad.top + pad.side) };
  };
  const layoutOf = (id: string) => {
    let l = laid.get(id);
    if (!l) {
      const kids = view.nodes.filter((c) => parentOf(tree, c) === id);
      laid.set(id, (l = layOut(kids, kept, sizeOf, spacing, containerDirection(depthOf(id)))));
    }
    return l;
  };

  for (const id of view.nodes) {
    const w = widthOf(id);
    const node: ElkNode = hasVisibleChild.has(id)
      ? {
          id,
          children: [],
          layoutOptions: {
            ...(separate ? { ...layered, 'elk.direction': layoutOf(id).dir } : {}),
            'elk.padding': `[top=${pad.top},left=${pad.side},bottom=${pad.side},right=${pad.side}]`,
            'elk.nodeSize.constraints': 'MINIMUM_SIZE',
            'elk.nodeSize.minimum': `(${w}, ${BOX.header + 16})`,
          },
        }
      : { id, width: w, height: leafHeight(id) };
    elkById.set(id, node);
  }
  for (const id of view.nodes) {
    const p = parentOf(tree, id);
    const parent = p !== null ? elkById.get(p) : undefined;
    (parent ?? root).children!.push(elkById.get(id)!);
  }

  const edges: ElkExtendedEdge[] = [];
  view.edges.forEach((e, i) => {
    if (!elkById.has(e.from) || !elkById.has(e.to) || !drawable(tree, e)) return;
    edges.push({ id: `e${i}`, sources: [e.from], targets: [e.to] });
  });
  root.edges = edges;
  return root;
}
