import type { EdgeKind, View, ViewEdge } from './types';

export function edgesOf(view: View, id: string): { outgoing: ViewEdge[]; incoming: ViewEdge[] } {
  const byCount = (a: ViewEdge, b: ViewEdge) => b.count - a.count;
  return {
    outgoing: view.edges.filter((e) => e.from === id).sort(byCount),
    incoming: view.edges.filter((e) => e.to === id).sort(byCount),
  };
}

export function breakdown(kinds: Partial<Record<EdgeKind, number>>): string {
  return Object.entries(kinds)
    .filter(([, n]) => (n ?? 0) > 0)
    .sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0))
    .map(([k, n]) => `${k} ${n}`)
    .join(', ');
}
