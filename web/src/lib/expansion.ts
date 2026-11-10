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

const canShow = (shown: ReadonlySet<string> | undefined, id: string) => shown === undefined || shown.has(id);

/** `id` has a child of kind `package` that the view can show. */
function hasPackageChild(tree: Tree, id: string, shown?: ReadonlySet<string>): boolean {
  return (tree.children.get(id) ?? []).some((c) => tree.byId.get(c)?.kind === 'package' && canShow(shown, c));
}

/**
 * Expanded set that makes every node of the level's kind visible and nothing deeper. With the
 * kind fold (ADR 0019) the level `packages` also opens every package with a package child, so
 * that the whole package tree shows; `shown` as in `splitByLevel`.
 */
export function levelExpanded(tree: Tree, level: Level, shown?: ReadonlySet<string>): Set<string> {
  const kinds = KINDS_OPEN_AT[level];
  const out = new Set<string>();
  for (const [id, n] of tree.byId) {
    if (!tree.children.has(id)) continue;
    if (kinds.includes(n.kind) || (level === 'packages' && n.kind === 'package' && hasPackageChild(tree, id, shown))) out.add(id);
  }
  return out;
}

/**
 * A container that the kind fold folds (ADR 0019): of the children that the view can show, one
 * is a package and one is not. The cut drops test code before it folds.
 */
function mixed(tree: Tree, id: string, shown?: ReadonlySet<string>): boolean {
  const kids = (tree.children.get(id) ?? []).filter((c) => canShow(shown, c)).map((c) => tree.byId.get(c)!);
  return kids.some((k) => k.kind === 'package') && kids.some((k) => k.kind !== 'package');
}

/** Unfolded set of a level: from `types` on, every container that the kind fold would fold. */
export function levelUnfolded(tree: Tree, level: Level, shown?: ReadonlySet<string>): Set<string> {
  if (level === 'modules' || level === 'packages') return new Set();
  return new Set([...tree.children.keys()].filter((id) => mixed(tree, id, shown)));
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
  unfolded: ReadonlySet<string> = new Set(),
): { level: Level; open: string[]; unfold: string[] } {
  const within = (base: Set<string>, set: ReadonlySet<string>) => [...base].every((id) => set.has(id) || !canShow(shown, id));
  let best = { level: 'modules' as Level, base: new Set<string>(), baseUnfolded: new Set<string>() };
  for (const level of LEVELS) {
    const base = levelExpanded(tree, level, shown);
    const baseUnfolded = levelUnfolded(tree, level, shown);
    if (within(base, expanded) && within(baseUnfolded, unfolded)) best = { level, base, baseUnfolded };
  }
  return {
    level: best.level,
    open: [...expanded].filter((id) => !best.base.has(id)),
    unfold: [...unfolded].filter((id) => !best.baseUnfolded.has(id)),
  };
}

/**
 * Depth state: the open containers, and the containers that show all children in spite of the
 * kind fold (ADR 0019) or of the focus (ADR 0011).
 */
export interface Depth {
  expanded: ReadonlySet<string>;
  unfolded: ReadonlySet<string>;
}

/** Prefix of a placeholder of the focus cut (ADR 0011): `rest:<container id>`. */
export const REST = 'rest:';

/** Parent of a graph node or of a placeholder, which is a view node without a graph node. */
export function parentOf(tree: Tree, id: string): string | null {
  const n = tree.byId.get(id);
  if (n) return n.parent;
  return id.startsWith(REST) ? id.slice(REST.length) : null;
}

/**
 * Children of `c` that the view can show but does not: the children folded into `rest:<c>`.
 * `shown` as in `splitByLevel`; without it every child can show.
 */
export function foldedChildren(tree: Tree, visible: ReadonlySet<string>, c: string, shown?: ReadonlySet<string>): GraphNode[] {
  return (tree.children.get(c) ?? [])
    .filter((id) => !visible.has(id) && (shown === undefined || shown.has(id)))
    .map((id) => tree.byId.get(id)!);
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
  view: { nodes: readonly string[]; expandable: readonly string[] },
  depth: Depth,
  shown?: ReadonlySet<string>,
): Depth & { expanded: Set<string>; unfolded: Set<string> } {
  const { expanded, unfolded } = depth;
  const frontier = view.expandable.filter((id) => !id.startsWith(REST) && !expanded.has(id));
  const ranks = frontier.map((id) => rank(tree, id, shown));
  const folds = kindFolds(tree, view.nodes, shown).filter((f) => !unfolded.has(f.container));
  const coarsest = Math.min(...ranks, ...folds.map((f) => f.rank));
  return {
    expanded: new Set([...expanded, ...frontier.filter((_, i) => ranks[i] === coarsest)]),
    unfolded: new Set([...unfolded, ...folds.filter((f) => f.rank === coarsest).map((f) => f.container)]),
  };
}

/**
 * The placeholders of the view that the kind fold made (ADR 0019), with the rank of the coarsest
 * kind of their folded children. A placeholder whose container shows a child that is not a package,
 * or folds a package, comes from the focus (ADR 0011) and is left out.
 */
function kindFolds(tree: Tree, nodes: readonly string[], shown?: ReadonlySet<string>): { container: string; rank: number }[] {
  const visible = new Set(nodes);
  const out: { container: string; rank: number }[] = [];
  for (const id of nodes) {
    if (tree.byId.has(id) || !id.startsWith(REST)) continue;
    const c = id.slice(REST.length);
    const folded = foldedChildren(tree, visible, c, shown);
    const kids = tree.children.get(c) ?? [];
    const showsOther = kids.some((k) => visible.has(k) && tree.byId.get(k)?.kind !== 'package');
    if (folded.length === 0 || showsOther || folded.some((f) => f.kind === 'package')) continue;
    out.push({ container: c, rank: Math.min(...folded.map((f) => KIND_RANK[f.kind])) });
  }
  return out;
}

/**
 * One layer less: closes every open container that has no open descendant. Open ids below a
 * closed container are not shown by the cut and are dropped first, so each step changes the view.
 */
export function shallower(tree: Tree, depth: Depth, shown?: ReadonlySet<string>): Depth {
  const { expanded } = depth;
  const parent = (id: string) => tree.byId.get(id)?.parent ?? null;
  const reached = (id: string) => {
    for (let p = parent(id); p !== null; p = parent(p)) if (!expanded.has(p)) return false;
    return true;
  };
  // An open container that the view cannot show (test code) does not keep its parent open.
  const open = [...expanded].filter((id) => reached(id) && canShow(shown, id));
  const inner = new Set(open.map(parent).filter((p) => p !== null));
  const leaves = open.filter((id) => !inner.has(id));
  // The kind fold (ADR 0019): a container that shows its other children as fine as the closed
  // layer folds them again, as "+" unfolded it together with that layer.
  const layer = Math.min(...leaves.map((id) => rank(tree, id, shown)));
  const otherRank = (id: string) =>
    Math.min(
      ...(tree.children.get(id) ?? [])
        .map((c) => tree.byId.get(c)!)
        .filter((n) => n.kind !== 'package' && canShow(shown, n.id))
        .map((n) => KIND_RANK[n.kind]),
    );
  return {
    expanded: new Set(open.filter((id) => inner.has(id))),
    unfolded: new Set([...depth.unfolded].filter((id) => otherRank(id) < layer)),
  };
}

/** A "+" step: the depth before it and the one it gave. */
export interface DepthStep {
  before: Depth;
  after: Depth;
}

const sameSet = (a: ReadonlySet<string>, b: ReadonlySet<string>) => a.size === b.size && [...a].every((x) => b.has(x));
const sameDepth = (a: Depth, b: Depth) => sameSet(a.expanded, b.expanded) && sameSet(a.unfolded, b.unfolded);

/**
 * "−": undoes the last "+" when the depth is still the one it gave. Otherwise the history is
 * stale and dropped, and the leaf rule of `shallower` applies.
 */
export function stepBack(
  tree: Tree,
  steps: readonly DepthStep[],
  depth: Depth,
  shown?: ReadonlySet<string>,
): { depth: Depth; steps: DepthStep[] } {
  const last = steps.at(-1);
  if (last && sameDepth(last.after, depth)) return { depth: last.before, steps: steps.slice(0, -1) };
  return { depth: shallower(tree, depth, shown), steps: [] };
}

/** Records a "+" step. A history that does not end at its start is stale and dropped. */
export function stepForward(steps: readonly DepthStep[], before: Depth, after: Depth): DepthStep[] {
  const last = steps.at(-1);
  const kept = last && sameDepth(last.after, before) ? steps : [];
  return [...kept, { before, after }];
}

/** Above this many visible boxes "+" asks first: the layout of 892 boxes takes about 2 s. */
export const BOX_LIMIT = 1500;

/** "+" to `boxes` visible boxes waits for a second "+" above the limit. */
export function mustConfirm(boxes: number, asked: boolean): boolean {
  return boxes > BOX_LIMIT && !asked;
}
