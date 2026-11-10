import type { ElkExtendedEdge, ElkNode } from 'elkjs/lib/elk-api';
import type { Tree } from './expansion';
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

export const ROOT_OPTIONS: Record<string, string> = {
  'elk.algorithm': 'layered',
  'elk.direction': 'RIGHT',
  'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
  'elk.edgeRouting': 'ORTHOGONAL',
  'elk.spacing.nodeNode': '14',
  'elk.layered.spacing.nodeNodeBetweenLayers': '28',
  'elk.spacing.edgeNode': '10',
  'elk.spacing.edgeEdge': '6',
  'elk.layered.mergeEdges': 'true',
  'elk.json.edgeCoords': 'ROOT',
  'elk.json.shapeCoords': 'ROOT',
};

/** Edge ids are `e<index into view.edges>`. */
export function edgeIndex(elkEdgeId: string): number {
  return Number(elkEdgeId.slice(1));
}

function isAncestor(tree: Tree, anc: string, id: string): boolean {
  for (let p = tree.byId.get(id)?.parent ?? null; p !== null; p = tree.byId.get(p)?.parent ?? null) {
    if (p === anc) return true;
  }
  return false;
}

export function toElkGraph(view: View, tree: Tree): ElkNode {
  const root: ElkNode = { id: 'root', layoutOptions: ROOT_OPTIONS, children: [], edges: [] };
  const visible = new Set(view.nodes);
  const expandable = new Set(view.expandable);
  const elkById = new Map<string, ElkNode>();
  const hasVisibleChild = new Set<string>();
  for (const id of view.nodes) {
    const p = tree.byId.get(id)?.parent;
    if (p != null && visible.has(p)) hasVisibleChild.add(p);
  }

  for (const id of view.nodes) {
    const name = tree.label.get(id) ?? id;
    const w = labelWidth(name, expandable.has(id));
    const node: ElkNode = hasVisibleChild.has(id)
      ? {
          id,
          children: [],
          layoutOptions: {
            'elk.padding': `[top=${BOX.header + 6},left=8,bottom=8,right=8]`,
            'elk.nodeSize.constraints': 'MINIMUM_SIZE',
            'elk.nodeSize.minimum': `(${w}, ${BOX.header + 16})`,
          },
        }
      : { id, width: w, height: BOX.height };
    elkById.set(id, node);
  }
  for (const id of view.nodes) {
    const p = tree.byId.get(id)?.parent;
    const parent = p != null ? elkById.get(p) : undefined;
    (parent ?? root).children!.push(elkById.get(id)!);
  }

  const edges: ElkExtendedEdge[] = [];
  view.edges.forEach((e, i) => {
    if (!elkById.has(e.from) || !elkById.has(e.to) || e.from === e.to) return;
    if (isAncestor(tree, e.from, e.to) || isAncestor(tree, e.to, e.from)) return;
    edges.push({ id: `e${i}`, sources: [e.from], targets: [e.to] });
  });
  root.edges = edges;
  return root;
}
