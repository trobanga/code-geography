import { drawable, parentOf, type Tree } from './expansion';
import { background } from './focus';
import { impliedLines } from './implied';
import { EDGE_KINDS, type EdgeKind, type ViewEdge } from './types';

/** The edge carries at least one added or removed graph edge (ADR 0009). */
export const changed = (e: ViewEdge) => e.added > 0 || e.removed > 0;

export type KindSwitch = Record<EdgeKind, boolean>;

/**
 * O2: effective count = sum of enabled kinds; drop edges at 0. The view has no per-kind split
 * of `added`/`removed`, so when a kind is switched off they are clipped to the new count (an
 * upper bound).
 */
export function filterEdges(edges: ViewEdge[], on: KindSwitch): ViewEdge[] {
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
    if (count === 0) continue;
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
  /** Edges at a placeholder that would have been lifted: background, not drawn (ADR 0011). */
  dropped: number;
}

/** Path from the top-level ancestor down to `id`. */
function pathTo(tree: Tree, id: string): string[] {
  const out: string[] = [];
  for (let n: string | null = id; n !== null; n = parentOf(tree, n)) out.push(n);
  return out.reverse();
}

/**
 * O3: draw each edge between the children of the lowest common ancestor of its ends. With
 * `dropBackground` (an active focus) a placeholder line that would be lifted is dropped; without
 * it `rest:<c>` lifts like a child of `c`, so that a package keeps the lines of its own types.
 */
export function liftEdges(edges: ViewEdge[], tree: Tree, dropBackground = true): Lifted {
  const merged = new Map<string, ViewEdge>();
  const badges = new Map<string, Badge>();
  const lifted: ViewEdge[] = [];
  let dropped = 0;
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
    if (from !== e.from || to !== e.to) {
      // A placeholder line merged into a real bundle would read as a dependency.
      if (dropBackground && background(tree, e)) {
        dropped++;
        continue;
      }
      lifted.push(e);
    }
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
  return { edges: [...merged.values()], badges, lifted, dropped };
}

export interface EdgeOptions {
  /** O1: paint only edges at the selected or hovered node. */
  onSelection: boolean;
  /** O2 */
  kinds: KindSwitch;
  /** O3 */
  lift: boolean;
  /** Commit overlay: paint only changed edges. */
  changesOnly: boolean;
  /** With `changesOnly`: lay out the changed boxes together and the others next to them. */
  changesTogether: boolean;
  /** Hide implied lines (a path shows them); null: auto, on above `IMPLIED_AUTO_FROM` lines. */
  implied: boolean | null;
}

export const DEFAULT_EDGE_OPTIONS: EdgeOptions = Object.freeze({
  onSelection: false,
  kinds: Object.freeze({ extends: true, implements: true, imports: true, type_ref: true, calls: true, value_ref: true }),
  lift: true,
  changesOnly: false,
  changesTogether: false,
  implied: null,
});

/** A line that hide implied never hides: a change of the commit. */
export const keepsLine = (e: ViewEdge) => changed(e);

/** In auto, hide implied is on when the view would draw more lines than this without it. */
export const IMPLIED_AUTO_FROM = 100;

export interface Drawn extends Lifted {
  /** Drawable view edges before any option. */
  total: number;
  /** The lines that hide implied took out of `edges`: not laid out, drawn only at the selection. */
  implied: ViewEdge[];
  /** Whether hide implied is in effect, by choice or by auto. */
  hideImplied: boolean;
}

/**
 * Drawable view edges → O2 → O3 → hide implied. O4 is in the WASM view, O1 only decides what is painted.
 * `together`: changes together is in effect (a commit is shown, changes only is on). Then only the
 * changed lines stay, so that the layout keeps no room for lines that are not painted.
 */
export function edgePipeline(viewEdges: ViewEdge[], tree: Tree, o: EdgeOptions, focused = false, together = false): Drawn {
  const d = allLines(viewEdges, tree, o, focused);
  return together ? { ...d, edges: d.edges.filter(changed), implied: d.implied.filter(changed) } : d;
}

function allLines(viewEdges: ViewEdge[], tree: Tree, o: EdgeOptions, focused: boolean): Drawn {
  const all = viewEdges.filter((e) => drawable(tree, e));
  const kept = filterEdges(all, o.kinds);
  const l = o.lift ? liftEdges(kept, tree, focused) : { edges: kept, badges: new Map<string, Badge>(), lifted: [], dropped: 0 };
  const hideImplied = o.implied ?? l.edges.length > IMPLIED_AUTO_FROM;
  if (!hideImplied) return { ...l, total: all.length, implied: [], hideImplied };
  const hidden = impliedLines(l.edges, keepsLine);
  return {
    ...l,
    edges: l.edges.filter((_, i) => !hidden.has(i)),
    implied: l.edges.filter((_, i) => hidden.has(i)),
    total: all.length,
    hideImplied,
  };
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

/** Readout of the edge bar: painted lines / drawable view edges, plus dropped background lines and implied lines. */
export function linesText(painted: number, total: number, dropped: number, implied = 0): string {
  return (
    `lines: ${painted} / ${total}` +
    (dropped > 0 ? `, ${dropped} background hidden` : '') +
    (implied > 0 ? `, ${implied} hidden as implied` : '')
  );
}
