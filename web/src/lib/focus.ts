import { pathToOpen } from './commit';
import { foldedChildren, REST, type Tree } from './expansion';
import type { NodeKind, View } from './types';

/** Focus of the cut (ADR 0011): what the WASM `focusView` folds around. */
export interface Focus {
  nodes: string[];
  /** Marked nodes and the ends of marked edges are of interest. */
  changes: boolean;
  /** Nodes one edge away from the set of interest are of interest too. */
  neighbors: boolean;
}

export const NO_FOCUS: Focus = Object.freeze({ nodes: [], changes: false, neighbors: false });

/** The container of a placeholder `rest:<id>`, or null for any other id. */
export function restOf(tree: Tree, id: string): string | null {
  return tree.byId.has(id) || !id.startsWith(REST) ? null : id.slice(REST.length);
}

const PLURAL: Record<NodeKind, [string, string]> = {
  module: ['module', 'modules'],
  package: ['package', 'packages'],
  type: ['type', 'types'],
  function: ['function', 'functions'],
};
const KIND_ORDER: NodeKind[] = ['module', 'package', 'type', 'function'];

/**
 * Label of a view node: the tree label, or for a placeholder the kinds of its folded children,
 * e.g. `3 types, 2 functions`. Without the visible nodes of the view: `N more`.
 */
export function labelOf(tree: Tree, view: Pick<View, 'rest'> & { nodes?: string[] }, id: string, shown?: ReadonlySet<string>): string {
  const c = restOf(tree, id);
  if (c === null) return tree.label.get(id) ?? id;
  const folded = view.nodes ? foldedChildren(tree, new Set(view.nodes), c, shown) : [];
  if (folded.length === 0) return `${view.rest?.[c] ?? 0} more`;
  return KIND_ORDER.flatMap((k) => {
    const n = folded.filter((f) => f.kind === k).length;
    return n === 0 ? [] : [`${n} ${PLURAL[k][n === 1 ? 0 : 1]}`];
  }).join(', ');
}

/** A focus without explicit nodes and without changes has an empty set of interest: the plain cut. */
export function focusActive(f: Focus): boolean {
  return f.nodes.length > 0 || f.changes;
}

/**
 * "Focus on this": the node and all that it touches. Opens only the containers on the path to
 * it, so that the neighbors show as their closed ancestors; an open container without anything
 * of interest would show all its children.
 */
export function focusOn(tree: Tree, id: string): { expanded: Set<string>; focus: Focus } {
  return {
    expanded: new Set(pathToOpen(tree, id)),
    focus: { nodes: [id], changes: false, neighbors: true },
  };
}

/** An edge at a placeholder summarizes unrelated nodes and is drawn as background (ADR 0011). */
export function background(tree: Tree, e: { from: string; to: string }): boolean {
  return restOf(tree, e.from) !== null || restOf(tree, e.to) !== null;
}
