import type { Tree } from './expansion';
import type { ChangeCount, Graph, NodeChange, ViewEdge } from './types';

/** The node or one of its ancestors is test code; the WASM view hides such nodes. */
export function inTestCode(tree: Tree, id: string): boolean {
  for (let n = tree.byId.get(id); n; n = n.parent === null ? undefined : tree.byId.get(n.parent)) {
    if (n.test) return true;
  }
  return false;
}

/** Ids of the marked nodes that the test-code switch shows, in tree order (depth first). */
function markedIds(tree: Tree, tests: boolean, root: string | null = null): string[] {
  const out: string[] = [];
  const roots = root === null ? [...tree.byId.values()].filter((n) => n.parent === null).map((n) => n.id) : [root];
  const walk = (id: string) => {
    const n = tree.byId.get(id);
    if (!n || (!tests && n.test)) return;
    if (n.change && id !== root) out.push(id);
    for (const c of tree.children.get(id) ?? []) walk(c);
  };
  for (const r of roots) if (tests || !inTestCode(tree, r)) walk(r);
  return out;
}

/** Containers on the path to `id`, top down, without `id`: what must be expanded to show it. */
export function pathToOpen(tree: Tree, id: string): string[] {
  const out: string[] = [];
  for (let p = tree.byId.get(id)?.parent ?? null; p !== null; p = tree.byId.get(p)?.parent ?? null) out.push(p);
  return out.reverse();
}

/** Expanded set that shows every marked node (outside test code unless `tests`) and opens nothing else. */
export function openChanged(tree: Tree, tests: boolean): Set<string> {
  const out = new Set<string>();
  for (const id of markedIds(tree, tests)) for (const p of pathToOpen(tree, id)) out.add(p);
  return out;
}

/**
 * The next set for "open changed": down to the changed types, with the types closed, so that a
 * changed type is one box with its badge. If `expanded` is already that set, the changed types
 * open also (the set of `openChanged`).
 */
export function openChangedStep(tree: Tree, tests: boolean, expanded: ReadonlySet<string>): Set<string> {
  const all = openChanged(tree, tests);
  const closed = new Set([...all].filter((id) => tree.byId.get(id)?.kind !== 'type'));
  const shown = closed.size === expanded.size && [...closed].every((id) => expanded.has(id));
  return shown ? all : closed;
}

export interface CommitTotals {
  nodes: ChangeCount;
  edges: { added: number; removed: number };
  /** Marked nodes and changed edges that the test-code switch hides. */
  hidden: { nodes: number; edges: number };
}

/** Totals for the commit strip, for what the test-code switch shows. */
export function commitTotals(graph: Graph, tree: Tree, tests: boolean): CommitTotals {
  const out: CommitTotals = {
    nodes: { added: 0, removed: 0, modified: 0 },
    edges: { added: 0, removed: 0 },
    hidden: { nodes: 0, edges: 0 },
  };
  for (const n of graph.nodes) {
    if (!n.change) continue;
    if (!tests && inTestCode(tree, n.id)) out.hidden.nodes++;
    else out.nodes[n.change]++;
  }
  for (const e of graph.edges) {
    if (!e.change) continue;
    if (!tests && (inTestCode(tree, e.from) || inTestCode(tree, e.to))) out.hidden.edges++;
    else out.edges[e.change]++;
  }
  return out;
}

/**
 * Marked nodes that the test-code switch shows, grouped by top-level ancestor (module), in tree
 * order. With `below`, only the nodes strictly below that node.
 */
export function markedByModule(
  tree: Tree,
  tests: boolean,
  below: string | null = null,
): { module: string; nodes: string[] }[] {
  const groups = new Map<string, string[]>();
  for (const id of markedIds(tree, tests, below)) {
    const m = pathToOpen(tree, id)[0] ?? id;
    const g = groups.get(m);
    if (g) g.push(id);
    else groups.set(m, [id]);
  }
  return [...groups].map(([module, nodes]) => ({ module, nodes }));
}

/** Label of a changed view edge: `+2`, `−1`, `+2 −1`; empty for an unchanged one. */
export function changeLabel(e: Pick<ViewEdge, 'added' | 'removed'>): string {
  const parts: string[] = [];
  if (e.added > 0) parts.push(`+${e.added}`);
  if (e.removed > 0) parts.push(`−${e.removed}`);
  return parts.join(' ');
}

/** Badge of a box: marked nodes in its subtree (from the view) without the node's own mark. */
export function badgeLabel(c: ChangeCount, own?: NodeChange): string {
  const n = (k: NodeChange) => c[k] - (own === k ? 1 : 0);
  const parts: string[] = [];
  if (n('added') > 0) parts.push(`+${n('added')}`);
  if (n('removed') > 0) parts.push(`−${n('removed')}`);
  if (n('modified') > 0) parts.push(`~${n('modified')}`);
  return parts.join(' ');
}

/** The plain graph of the head of an overlay: without what the head removed and without marks. */
export function headOf(overlay: Graph): Graph {
  const plain = <T extends { change?: unknown }>({ change: _, ...rest }: T) => rest;
  return {
    nodes: overlay.nodes.filter((n) => n.change !== 'removed').map(plain),
    edges: overlay.edges.filter((e) => e.change !== 'removed').map(plain),
  };
}
