import { drawable, parentOf, type Tree } from './expansion';
import { EDGE_KINDS, type EdgeKind, type ViewEdge } from './types';

/** The edge carries at least one added or removed graph edge (ADR 0009). */
export const changed = (e: ViewEdge) => e.added > 0 || e.removed > 0;

export type KindSwitch = Record<EdgeKind, boolean>;

/**
 * O2: effective count = sum of enabled kinds; drop edges at 0 or below `min`, except changed
 * edges when `keepChanged`. The view has no per-kind split of `added`/`removed`, so when a
 * kind is switched off they are clipped to the new count (an upper bound).
 */
export function filterEdges(edges: ViewEdge[], on: KindSwitch, min: number, keepChanged = false): ViewEdge[] {
  const out: ViewEdge[] = [];
  for (const e of edges) {
    const kinds: ViewEdge['kinds'] = {};
    let count = 0;
    for (const k of EDGE_KINDS) {
      const n = e.kinds[k] ?? 0;
      if (on[k] && n > 0) {
        kinds[k] = n;
        count += n;
      }
    }
    if (count === 0 || (count < min && !(keepChanged && changed(e)))) continue;
    out.push({ from: e.from, to: e.to, count, kinds, added: Math.min(e.added, count), removed: Math.min(e.removed, count) });
  }
  return out;
}

export interface Badge {
  out: number;
  in: number;
}
export interface Lifted {
  edges: ViewEdge[];
  /** Per visible node: summed counts of its own edges that were lifted to an ancestor. */
  badges: Map<string, Badge>;
  /** The input edges whose ends were replaced. */
  lifted: ViewEdge[];
}

/** Path from the top-level ancestor down to `id`. */
function pathTo(tree: Tree, id: string): string[] {
  const out: string[] = [];
  for (let n: string | null = id; n !== null; n = parentOf(tree, n)) out.push(n);
  return out.reverse();
}

/** O3: draw each edge between the children of the lowest common ancestor of its ends. */
export function liftEdges(edges: ViewEdge[], tree: Tree): Lifted {
  const merged = new Map<string, ViewEdge>();
  const badges = new Map<string, Badge>();
  const lifted: ViewEdge[] = [];
  const badge = (id: string) => {
    let b = badges.get(id);
    if (!b) badges.set(id, (b = { out: 0, in: 0 }));
    return b;
  };
  for (const e of edges) {
    const pa = pathTo(tree, e.from);
    const pb = pathTo(tree, e.to);
    let i = 0;
    while (i < pa.length && i < pb.length && pa[i] === pb[i]) i++;
    // One end is an ancestor of the other: no child of the LCA on that side.
    const from = i < pa.length && i < pb.length ? pa[i] : e.from;
    const to = i < pa.length && i < pb.length ? pb[i] : e.to;
    if (from !== e.from || to !== e.to) lifted.push(e);
    if (from !== e.from) badge(e.from).out += e.count;
    if (to !== e.to) badge(e.to).in += e.count;
    const key = `${from}\u0000${to}`;
    const m = merged.get(key);
    if (!m) {
      merged.set(key, { from, to, count: e.count, kinds: { ...e.kinds }, added: e.added, removed: e.removed });
      continue;
    }
    m.count += e.count;
    m.added += e.added;
    m.removed += e.removed;
    for (const k of EDGE_KINDS) {
      const n = e.kinds[k];
      if (n) m.kinds[k] = (m.kinds[k] ?? 0) + n;
    }
  }
  return { edges: [...merged.values()], badges, lifted };
}

/** Strongly connected components (Tarjan); returns component index per node. */
function components(nodes: string[], succ: Map<string, string[]>): Map<string, number> {
  const index = new Map<string, number>();
  const low = new Map<string, number>();
  const comp = new Map<string, number>();
  const stack: string[] = [];
  const onStack = new Set<string>();
  let next = 0;
  let count = 0;
  const visit = (v: string) => {
    index.set(v, next);
    low.set(v, next++);
    stack.push(v);
    onStack.add(v);
    for (const w of succ.get(v) ?? []) {
      if (!index.has(w)) {
        visit(w);
        low.set(v, Math.min(low.get(v)!, low.get(w)!));
      } else if (onStack.has(w)) low.set(v, Math.min(low.get(v)!, index.get(w)!));
    }
    if (low.get(v) === index.get(v)) {
      let w: string;
      do {
        w = stack.pop()!;
        onStack.delete(w);
        comp.set(w, count);
      } while (w !== v);
      count++;
    }
  };
  for (const v of nodes) if (!index.has(v)) visit(v);
  return comp;
}

/**
 * O5: hide a→c when c is also reached by a path of two or more edges. Works on the
 * condensation, so edges inside a strongly connected component are never hidden and
 * reachability is kept.
 */
export function reduceTransitive(edges: ViewEdge[]): ViewEdge[] {
  const succ = new Map<string, string[]>();
  const nodes = new Set<string>();
  for (const e of edges) {
    nodes.add(e.from);
    nodes.add(e.to);
    const l = succ.get(e.from);
    if (l) l.push(e.to);
    else succ.set(e.from, [e.to]);
  }
  const comp = components([...nodes], succ);
  const csucc = new Map<number, Set<number>>();
  for (const e of edges) {
    const a = comp.get(e.from)!;
    const b = comp.get(e.to)!;
    if (a === b) continue;
    let s = csucc.get(a);
    if (!s) csucc.set(a, (s = new Set()));
    s.add(b);
  }
  const reach = new Map<number, Set<number>>();
  const reachOf = (c: number): Set<number> => {
    let r = reach.get(c);
    if (r) return r;
    r = new Set();
    for (const d of csucc.get(c) ?? []) {
      r.add(d);
      for (const x of reachOf(d)) r.add(x);
    }
    reach.set(c, r);
    return r;
  };
  return edges.filter((e) => {
    const a = comp.get(e.from)!;
    const b = comp.get(e.to)!;
    if (a === b || changed(e)) return true;
    for (const x of csucc.get(a) ?? []) if (x !== b && reachOf(x).has(b)) return false;
    return true;
  });
}

export interface EdgeOptions {
  /** O1: paint only edges at the selected or hovered node. */
  onSelection: boolean;
  /** O2 */
  kinds: KindSwitch;
  min: number;
  /** O3 */
  lift: boolean;
  /** O4: passed to the WASM view. */
  mergeImports: boolean;
  /** O5 */
  reduce: boolean;
  /** Commit overlay: paint only changed edges; `min` does not drop them. */
  changesOnly: boolean;
}

export const MIN_COUNT_MAX = 10;

export const DEFAULT_EDGE_OPTIONS: EdgeOptions = Object.freeze({
  onSelection: false,
  kinds: Object.freeze({ extends: true, implements: true, imports: true, type_ref: true }),
  min: 1,
  lift: false,
  mergeImports: false,
  reduce: false,
  changesOnly: false,
});

export interface Drawn extends Lifted {
  /** Drawable view edges before any option. */
  total: number;
}

/** Drawable view edges → O2 → O3 → O5. O4 is in the WASM view, O1 only decides what is painted. */
export function edgePipeline(viewEdges: ViewEdge[], tree: Tree, o: EdgeOptions): Drawn {
  const all = viewEdges.filter((e) => drawable(tree, e));
  // Reads `changesOnly` only when it matters, so toggling it at min 1 does not re-run the layout.
  const kept = filterEdges(all, o.kinds, o.min, o.min > 1 && o.changesOnly);
  const l = o.lift ? liftEdges(kept, tree) : { edges: kept, badges: new Map<string, Badge>(), lifted: [] };
  return { ...l, edges: o.reduce ? reduceTransitive(l.edges) : l.edges, total: all.length };
}

/**
 * O1: with `onSelection`, paint only edges at the selected or the hovered node.
 * With `changesOnly`, paint only changed edges.
 */
export function painted(
  e: ViewEdge,
  onSelection: boolean,
  selected: string | null,
  hover: string | null,
  changesOnly = false,
): boolean {
  if (changesOnly && !changed(e)) return false;
  if (!onSelection) return true;
  const at = (id: string | null) => id !== null && (e.from === id || e.to === id);
  return at(selected) || at(hover);
}
