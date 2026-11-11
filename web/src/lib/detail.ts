import { parentOf, type Tree } from './expansion';
import type { EdgeKind, View, ViewEdge } from './types';

export function edgesOf(view: View, id: string): { outgoing: ViewEdge[]; incoming: ViewEdge[] } {
  const byCount = (a: ViewEdge, b: ViewEdge) => b.count - a.count;
  return {
    outgoing: view.edges.filter((e) => e.from === id).sort(byCount),
    incoming: view.edges.filter((e) => e.to === id).sort(byCount),
  };
}

/**
 * The dependencies that the side panel lists for the selected box. An open container has no view
 * edge of its own (the cut ends them at its children); it lists the lines that the diagram draws or
 * hides as implied at its box, `lines` after lift, which are sums over its contents (`summed`).
 */
export function boxLines(
  view: View,
  tree: Tree,
  lines: readonly ViewEdge[],
  id: string,
): { outgoing: ViewEdge[]; incoming: ViewEdge[]; summed: boolean } {
  const open = view.nodes.some((c) => c !== id && parentOf(tree, c) === id);
  if (!open) return { ...edgesOf(view, id), summed: false };
  return { ...edgesOf({ ...view, edges: [...lines] }, id), summed: true };
}

export function breakdown(kinds: Partial<Record<EdgeKind, number>>): string {
  return Object.entries(kinds)
    .filter(([, n]) => (n ?? 0) > 0)
    .sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0))
    .map(([k, n]) => `${k} ${n}`)
    .join(', ');
}
