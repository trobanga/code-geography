import { LEVELS, type GraphNode, type Level, type NodeKind } from './types';

export interface Tree {
  byId: Map<string, GraphNode>;
  children: Map<string, string[]>;
  /** Display label: the name; a top-level package loses the dot-prefix that all top-level packages share. */
  label: Map<string, string>;
}

/** A package that is not inside another package. A nested package has a name relative to its parent. */
function topPackages(nodes: GraphNode[]): Set<string> {
  const packages = new Set(nodes.filter((n) => n.kind === 'package').map((n) => n.id));
  return new Set(nodes.filter((n) => packages.has(n.id) && !(n.parent !== null && packages.has(n.parent))).map((n) => n.id));
}

function sharedPackagePrefix(nodes: GraphNode[], top: Set<string>): string[] {
  let prefix: string[] | null = null;
  for (const n of nodes) {
    if (!top.has(n.id)) continue;
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
  const top = topPackages(nodes);
  const prefix = sharedPackagePrefix(nodes, top);
  for (const n of nodes) {
    const rest = top.has(n.id) ? n.name.split('.').slice(prefix.length).join('.') : '';
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

const KIND_RANK: Record<NodeKind, number> = { module: 0, package: 1, type: 2, function: 3 };

/**
 * Rank of a container: the coarsest kind among its children that the view can show. A type has
 * the rank of a function, so that types with inner types open together with the other types.
 */
function rank(tree: Tree, id: string, shown: ReadonlySet<string> | undefined): number {
  let r = KIND_RANK.function;
  if (tree.byId.get(id)?.kind === 'type') return r;
  for (const c of tree.children.get(id) ?? []) {
    const n = tree.byId.get(c);
    if (n && (shown === undefined || shown.has(c))) r = Math.min(r, KIND_RANK[n.kind]);
  }
  return r;
}

/**
 * One layer deeper: of the containers that the view shows closed and that can open, those whose
 * children have the coarsest kind, so that package layers open one by one before types, and types
 * before functions. Takes the view's `expandable`, so that the cut stays in WASM and a focus cut
 * opens only what it shows; `shown` are the nodes the view can show (see `splitByLevel`).
 */
export function deeper(
  tree: Tree,
  expandable: readonly string[],
  expanded: ReadonlySet<string>,
  shown?: ReadonlySet<string>,
): Set<string> {
  const frontier = expandable.filter((id) => !id.startsWith(REST) && !expanded.has(id));
  const ranks = frontier.map((id) => rank(tree, id, shown));
  const coarsest = Math.min(...ranks);
  return new Set([...expanded, ...frontier.filter((_, i) => ranks[i] === coarsest)]);
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
