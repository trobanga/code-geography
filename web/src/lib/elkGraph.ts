import type { ElkExtendedEdge, ElkNode } from 'elkjs/lib/elk-api';
import { drawable, type Tree } from './expansion';
import type { View } from './types';

/** Geometry shared by the ELK input and the SVG renderer. */
export const BOX = {
  font: 11,
  charW: 6.7,
  height: 22,
  pad: 6,
  kindW: 14,
  markerW: 14,
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

function containerPadding(nodeCount: number): string {
  const f = sparseness(nodeCount);
  return `[top=${BOX.header + Number(lerp(6, 14, f))},left=${lerp(8, 24, f)},bottom=${lerp(8, 24, f)},right=${lerp(8, 24, f)}]`;
}

/** Edge ids are `e<index into view.edges>`. */
export function edgeIndex(elkEdgeId: string): number {
  return Number(elkEdgeId.slice(1));
}

export function toElkGraph(view: View, tree: Tree, nodeCount = view.nodes.length): ElkNode {
  const root: ElkNode = { id: 'root', layoutOptions: rootOptions(nodeCount), children: [], edges: [] };
  const visible = new Set(view.nodes);
  const expandable = new Set(view.expandable);
  const elkById = new Map<string, ElkNode>();
  const hasVisibleChild = new Set<string>();
  for (const id of view.nodes) {
    const p = tree.byId.get(id)?.parent;
    if (p != null && visible.has(p)) hasVisibleChild.add(p);
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

  for (const id of view.nodes) {
    const name = tree.label.get(id) ?? id;
    const w = labelWidth(name, expandable.has(id));
    const node: ElkNode = hasVisibleChild.has(id)
      ? {
          id,
          children: [],
          layoutOptions: {
            'elk.padding': containerPadding(nodeCount),
            'elk.nodeSize.constraints': 'MINIMUM_SIZE',
            'elk.nodeSize.minimum': `(${w}, ${BOX.header + 16})`,
          },
        }
      : { id, width: w, height: leafHeight(id) };
    elkById.set(id, node);
  }
  for (const id of view.nodes) {
    const p = tree.byId.get(id)?.parent;
    const parent = p != null ? elkById.get(p) : undefined;
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
