import { BOX } from './elkGraph';
import type { LaidNode, Point } from './layout';

export interface Offset {
  dx: number;
  dy: number;
}
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Room an ancestor keeps around a child that was moved to its border. */
export const GROW_PAD = { top: BOX.header + 6, side: 8 } as const;

const ZERO: Offset = Object.freeze({ dx: 0, dy: 0 });

/**
 * Applies manual offsets: a node moves by the sum of its own offset and those of its
 * ancestors. Then each container grows (never shrinks) to enclose its children.
 * `shift` is the total displacement of each node, before growth.
 */
export function placeNodes(
  nodes: LaidNode[],
  offsets: ReadonlyMap<string, Offset>,
): { nodes: LaidNode[]; shift: Map<string, Offset> } {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const shift = new Map<string, Offset>();
  const shiftOf = (id: string): Offset => {
    const known = shift.get(id);
    if (known) return known;
    const n = byId.get(id)!;
    const own = offsets.get(id) ?? ZERO;
    const up = n.parent !== null && byId.has(n.parent) ? shiftOf(n.parent) : ZERO;
    const s = { dx: own.dx + up.dx, dy: own.dy + up.dy };
    shift.set(id, s);
    return s;
  };
  if (offsets.size === 0) {
    for (const n of nodes) shift.set(n.id, ZERO);
    return { nodes, shift };
  }
  const placed = new Map<string, LaidNode>();
  const depth = new Map<string, number>();
  const depthOf = (id: string): number => {
    let d = depth.get(id);
    if (d !== undefined) return d;
    const p = byId.get(id)!.parent;
    d = p !== null && byId.has(p) ? depthOf(p) + 1 : 0;
    depth.set(id, d);
    return d;
  };
  for (const n of nodes) {
    const s = shiftOf(n.id);
    placed.set(n.id, { ...n, x: n.x + s.dx, y: n.y + s.dy });
  }
  // Deepest first, so a grown child is enclosed by its grown parent.
  for (const n of [...nodes].sort((a, b) => depthOf(b.id) - depthOf(a.id))) {
    if (n.parent === null || !placed.has(n.parent)) continue;
    const c = placed.get(n.id)!;
    const p = placed.get(n.parent)!;
    const x0 = Math.min(p.x, c.x - GROW_PAD.side);
    const y0 = Math.min(p.y, c.y - GROW_PAD.top);
    const x1 = Math.max(p.x + p.w, c.x + c.w + GROW_PAD.side);
    const y1 = Math.max(p.y + p.h, c.y + c.h + GROW_PAD.side);
    placed.set(n.parent, { ...p, x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
  }
  return { nodes: nodes.map((n) => placed.get(n.id)!), shift };
}

function clipFromCentre(r: Rect, dx: number, dy: number): Point {
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  const t = Math.min(dx === 0 ? Infinity : r.w / 2 / Math.abs(dx), dy === 0 ? Infinity : r.h / 2 / Math.abs(dy));
  return Number.isFinite(t) ? { x: cx + dx * t, y: cy + dy * t } : { x: cx, y: cy };
}

/** Straight line from the border of `a` to the border of `b`, along the line between their centres. */
export function borderLine(a: Rect, b: Rect): [Point, Point] {
  const dx = b.x + b.w / 2 - (a.x + a.w / 2);
  const dy = b.y + b.h / 2 - (a.y + a.h / 2);
  return [clipFromCentre(a, dx, dy), clipFromCentre(b, -dx, -dy)];
}

/** The ELK route if neither end moved, translated if both moved alike, else a border-to-border line. */
export function edgePoints(route: Point[], from: Offset, to: Offset, fromRect: Rect, toRect: Rect): Point[] {
  if (from.dx === to.dx && from.dy === to.dy) {
    if (from.dx === 0 && from.dy === 0) return route;
    return route.map((p) => ({ x: p.x + from.dx, y: p.y + from.dy }));
  }
  return borderLine(fromRect, toRect);
}
