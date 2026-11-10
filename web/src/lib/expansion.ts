import { LEVELS, type GraphNode, type Level, type NodeKind } from './types';

export interface Tree {
  byId: Map<string, GraphNode>;
  children: Map<string, string[]>;
  /** Display label: the name, for packages without the dot-prefix shared by all packages. */
  label: Map<string, string>;
}

function sharedPackagePrefix(nodes: GraphNode[]): string[] {
  let prefix: string[] | null = null;
  for (const n of nodes) {
    if (n.kind !== 'package') continue;
    const segs = n.name.split('.');
    if (prefix === null) prefix = segs;
    else {
      let i = 0;
      while (i < prefix.length && i < segs.length && prefix[i] === segs[i]) i++;
      prefix = prefix.slice(0, i);
    }
  }
  return prefix ?? [];
}

export function buildTree(nodes: GraphNode[]): Tree {
  const byId = new Map<string, GraphNode>();
  const children = new Map<string, string[]>();
  const label = new Map<string, string>();
  const prefix = sharedPackagePrefix(nodes);
  for (const n of nodes) {
    const rest = n.kind === 'package' ? n.name.split('.').slice(prefix.length).join('.') : '';
    label.set(n.id, rest || n.name);
    byId.set(n.id, n);
    if (n.parent !== null) {
      const list = children.get(n.parent);
      if (list) list.push(n.id);
      else children.set(n.parent, [n.id]);
    }
  }
  return { byId, children, label };
}

const KINDS_OPEN_AT: Record<Level, NodeKind[]> = {
  modules: [],
  packages: ['module'],
  types: ['module', 'package'],
  functions: ['module', 'package', 'type'],
};

/** Expanded set that makes every node of the level's kind visible and nothing deeper. */
export function levelExpanded(tree: Tree, level: Level): Set<string> {
  const kinds = KINDS_OPEN_AT[level];
  const out = new Set<string>();
  for (const [id, n] of tree.byId) {
    if (kinds.includes(n.kind) && tree.children.has(id)) out.add(id);
  }
  return out;
}

/** Expanded set without `id` and without any of its descendants. */
export function collapse(tree: Tree, expanded: ReadonlySet<string>, id: string): Set<string> {
  const out = new Set(expanded);
  const stack = [id];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    out.delete(cur);
    for (const c of tree.children.get(cur) ?? []) if (out.has(c)) stack.push(c);
  }
  return out;
}

/**
 * Deepest level whose expanded set is contained in `expanded`, and the ids beyond it. With
 * `shown`, the nodes that the view can show (from the WASM cut), a level container that the view
 * never shows, such as a package of test code only, does not count.
 */
export function splitByLevel(
  tree: Tree,
  expanded: ReadonlySet<string>,
  shown?: ReadonlySet<string>,
): { level: Level; open: string[] } {
  let best: { level: Level; base: Set<string> } = { level: 'modules', base: new Set() };
  for (const level of LEVELS) {
    const base = levelExpanded(tree, level);
    if ([...base].every((id) => expanded.has(id) || (shown !== undefined && !shown.has(id)))) best = { level, base };
  }
  return { level: best.level, open: [...expanded].filter((id) => !best.base.has(id)) };
}

/** Prefix of a placeholder of the focus cut (ADR 0011): `rest:<container id>`. */
export const REST = 'rest:';

/** Parent of a graph node or of a placeholder, which is a view node without a graph node. */
export function parentOf(tree: Tree, id: string): string | null {
  const n = tree.byId.get(id);
  if (n) return n.parent;
  return id.startsWith(REST) ? id.slice(REST.length) : null;
}

export function isAncestor(tree: Tree, anc: string, id: string): boolean {
  for (let p = parentOf(tree, id); p !== null; p = parentOf(tree, p)) {
    if (p === anc) return true;
  }
  return false;
}

/** `nodeCount` sets the spacing; defaults to the number of visible nodes. */

/** An edge can be drawn when its ends differ and neither contains the other (ADR 0006). */
export function drawable(tree: Tree, e: { from: string; to: string }): boolean {
  return e.from !== e.to && !isAncestor(tree, e.from, e.to) && !isAncestor(tree, e.to, e.from);
}

/**
 * One layer deeper: every container that the view shows closed and that can open. Takes the
 * view's `expandable`, so that the cut stays in WASM and a focus cut opens only what it shows.
 */
export function deeper(expandable: readonly string[], expanded: ReadonlySet<string>): Set<string> {
  const out = new Set(expanded);
  for (const id of expandable) if (!id.startsWith(REST)) out.add(id);
  return out;
}

/**
 * One layer less: closes every open container that has no open descendant. Open ids below a
 * closed container are not shown by the cut and are dropped first, so each step changes the view.
 */
export function shallower(tree: Tree, expanded: ReadonlySet<string>): Set<string> {
  const parent = (id: string) => tree.byId.get(id)?.parent ?? null;
  const reached = (id: string) => {
    for (let p = parent(id); p !== null; p = parent(p)) if (!expanded.has(p)) return false;
    return true;
  };
  const open = [...expanded].filter(reached);
  const inner = new Set(open.map(parent).filter((p) => p !== null));
  return new Set(open.filter((id) => inner.has(id)));
}

/** A "+" step: the expanded set before it and the one it gave. */
export interface DepthStep {
  before: ReadonlySet<string>;
  after: ReadonlySet<string>;
}

const sameSet = (a: ReadonlySet<string>, b: ReadonlySet<string>) => a.size === b.size && [...a].every((x) => b.has(x));

/**
 * "−": undoes the last "+" when the expanded set is still the one it gave. Otherwise the
 * history is stale and dropped, and the leaf rule of `shallower` applies.
 */
export function stepBack(
  tree: Tree,
  steps: readonly DepthStep[],
  expanded: ReadonlySet<string>,
): { expanded: Set<string>; steps: DepthStep[] } {
  const last = steps.at(-1);
  if (last && sameSet(last.after, expanded)) return { expanded: new Set(last.before), steps: steps.slice(0, -1) };
  return { expanded: shallower(tree, expanded), steps: [] };
}

/** Records a "+" step. A history that does not end at its start is stale and dropped. */
export function stepForward(steps: readonly DepthStep[], before: ReadonlySet<string>, after: ReadonlySet<string>): DepthStep[] {
  const last = steps.at(-1);
  const kept = last && sameSet(last.after, before) ? steps : [];
  return [...kept, { before, after }];
}

/** Above this many visible boxes "+" asks first: the layout of 892 boxes takes about 2 s. */
export const BOX_LIMIT = 1500;

/** "+" to `boxes` visible boxes waits for a second "+" above the limit. */
export function mustConfirm(boxes: number, asked: boolean): boolean {
  return boxes > BOX_LIMIT && !asked;
}
