import type { Rect } from './drag';
import type { Point } from './layout';

export interface Line {
  from: string;
  to: string;
}

type Side = 'L' | 'R' | 'T' | 'B';
type End = 'a' | 'b';
const ENDS: End[] = ['a', 'b'];

/** Largest distance between two attach points on one side. */
const PORT_GAP = 10;
/** Shortest first and last segment, so the arrow head has room. */
const STUB = 10;
/** Smallest distance between two attach points before a side hands lines to its neighbours. */
const MIN_GAP = 4;
/** Distance between the middle segments of a fan. */
const STEP = 8;

interface Plan {
  a: Rect;
  b: Rect;
  sa: Side;
  sb: Side;
  kind: Kind;
  /** Attach coordinate along the side: y on L/R, x on T/B. */
  pa: number;
  pb: number;
}

const cmp = (u: string, v: string) => (u < v ? -1 : u > v ? 1 : 0);
const keyOf = (l: Line) => `${l.from}\u0000${l.to}`;
const vertical = (s: Side) => s === 'L' || s === 'R';
/** Coordinate of the side across it: x of L/R, y of T/B. */
function across(r: Rect, s: Side): number {
  if (s === 'L') return r.x;
  if (s === 'R') return r.x + r.w;
  if (s === 'T') return r.y;
  return r.y + r.h;
}
/** Start and length of the side along it. */
const along = (r: Rect, s: Side): [number, number] => (vertical(s) ? [r.y, r.h] : [r.x, r.w]);
const centre = (r: Rect, s: Side) => {
  const [p, n] = along(r, s);
  return p + n / 2;
};
const point = (s: Side, c: number, p: number): Point => (vertical(s) ? { x: c, y: p } : { x: p, y: c });

/** Gap between two boxes below which a line cannot run between them. */
const CLEAR = 2 * STUB;

const holds = (o: Rect, i: Rect) => o.x <= i.x && o.y <= i.y && i.x + i.w <= o.x + o.w && i.y + i.h <= o.y + o.h;

/** `around`: boxes too close to run a line between them; it goes around their right sides. */
type Kind = 'between' | 'contain' | 'around';

function sides(a: Rect, b: Rect): [Side, Side, Kind] {
  if (a.x + a.w + CLEAR <= b.x) return ['R', 'L', 'between'];
  if (b.x + b.w + CLEAR <= a.x) return ['L', 'R', 'between'];
  if (a.y + a.h + CLEAR <= b.y) return ['B', 'T', 'between'];
  if (b.y + b.h + CLEAR <= a.y) return ['T', 'B', 'between'];
  // One box holds the other: from the left border of the outer box, or to its right border.
  if (holds(a, b)) return ['L', 'L', 'contain'];
  if (holds(b, a)) return ['R', 'R', 'contain'];
  if (a.x + a.w <= b.x) return ['R', 'L', 'between'];
  if (b.x + b.w <= a.x) return ['L', 'R', 'between'];
  return ['R', 'R', 'around'];
}

/**
 * Orthogonal polylines between the boxes of `lines`, in input order, for lines drawn without
 * ELK (the real edges of the selection, O3). The boxes do not move; lines may cross boxes.
 * Lines at one side of a box get distinct attach points, ordered by the position of their
 * other end, and a fan of lines gets distinct middle segments, so that they do not meet.
 */
export function orthoRoutes(lines: Line[], rects: ReadonlyMap<string, Rect>): Point[][] {
  const plans: Plan[] = lines.map((l) => {
    const a = rects.get(l.from)!;
    const b = rects.get(l.to)!;
    const [sa, sb, kind] = sides(a, b);
    return { a, b, sa, sb, kind, pa: centre(a, sa), pb: centre(b, sb) };
  });
  const rectOf = (i: number, e: End) => (e === 'a' ? plans[i].a : plans[i].b);
  const sideOf = (i: number, e: End) => (e === 'a' ? plans[i].sa : plans[i].sb);
  const keyAt = (i: number, e: End) => `${e === 'a' ? lines[i].from : lines[i].to}\u0000${sideOf(i, e)}`;
  const groupAll = () => {
    const groups = new Map<string, { i: number; e: End }[]>();
    plans.forEach((_, i) => {
      for (const e of ENDS) {
        const g = groups.get(keyAt(i, e));
        if (g) g.push({ i, e });
        else groups.set(keyAt(i, e), [{ i, e }]);
      }
    });
    return groups;
  };

  // A side too short for its lines hands those whose other end lies wholly above or below to
  // the top or bottom side; such a line bends once.
  const bent = new Set<number>();
  for (const g of [...groupAll().values()].sort((u, v) => v.length - u.length)) {
    const { i: i0, e: e0 } = g[0];
    if (!vertical(sideOf(i0, e0)) || along(rectOf(i0, e0), sideOf(i0, e0))[1] / (g.length + 1) >= MIN_GAP) continue;
    for (const { i, e } of g) {
      if (bent.has(i) || plans[i].kind !== 'between') continue;
      const hub = rectOf(i, e);
      const far = rectOf(i, e === 'a' ? 'b' : 'a');
      const side = far.y + far.h <= hub.y ? 'T' : far.y >= hub.y + hub.h ? 'B' : null;
      if (side === null) continue;
      if (e === 'a') plans[i].sa = side;
      else plans[i].sb = side;
      bent.add(i);
    }
  }

  // Attach points: per side of a box, ordered by the other end, then by line. Bent lines on a
  // top or bottom side come first from the left and last from the right, ordered so that they
  // do not cross each other.
  const groups = groupAll();
  const sortKey = (i: number, e: End): [number, number] => {
    const side = sideOf(i, e);
    const hub = rectOf(i, e);
    const far = rectOf(i, e === 'a' ? 'b' : 'a');
    if (!bent.has(i) || vertical(side)) return [1, centre(far, side)];
    const fy = far.y + far.h / 2;
    const left = far.x + far.w / 2 < hub.x + hub.w / 2;
    const sign = (side === 'T') === left ? -1 : 1;
    return [left ? 0 : 2, sign * fy];
  };
  for (const g of groups.values()) {
    const keys = new Map(g.map((v) => [v, sortKey(v.i, v.e)]));
    // Ties by line, the same on both ends, so that two lines between one pair run parallel.
    g.sort((u, v) => {
      const [ku, kv] = [keys.get(u)!, keys.get(v)!];
      return ku[0] - kv[0] || ku[1] - kv[1] || cmp(keyOf(lines[u.i]), keyOf(lines[v.i])) || u.i - v.i;
    });
    for (const [k, v] of g.entries()) {
      const [s, len] = along(rectOf(v.i, v.e), sideOf(v.i, v.e));
      const gap = Math.min(PORT_GAP, len / (g.length + 1));
      const pos = s + len / 2 + (k - (g.length - 1) / 2) * gap;
      if (v.e === 'a') plans[v.i].pa = pos;
      else plans[v.i].pb = pos;
    }
  }

  // Middle segments: a fan turns near its hub, the end whose side has more lines. Of the lines
  // that turn up (or down), the one whose attach point is nearest the turn turns first.
  const hubOf = plans.map((_, i): End => {
    const [ka, kb] = [keyAt(i, 'a'), keyAt(i, 'b')];
    const [na, nb] = [groups.get(ka)!.length, groups.get(kb)!.length];
    // On a tie both lines of a pair turn at the same box.
    return nb > na || (nb === na && kb < ka) ? 'b' : 'a';
  });
  const rank = new Map<number, number>();
  const fans = new Map<string, number[]>();
  hubOf.forEach((h, i) => {
    if (bent.has(i)) return;
    const key = keyAt(i, h);
    const f = fans.get(key);
    if (f) f.push(i);
    else fans.set(key, [i]);
  });
  for (const f of fans.values()) {
    const hubPort = (i: number) => (hubOf[i] === 'a' ? plans[i].pa : plans[i].pb);
    const farPort = (i: number) => (hubOf[i] === 'a' ? plans[i].pb : plans[i].pa);
    const up = f.filter((i) => farPort(i) < hubPort(i)).sort((i, j) => hubPort(i) - hubPort(j));
    const down = f.filter((i) => farPort(i) > hubPort(i)).sort((i, j) => hubPort(j) - hubPort(i));
    up.forEach((i, k) => rank.set(i, k));
    down.forEach((i, k) => rank.set(i, k));
  }

  return plans.map((p, i) => {
    const ca = across(p.a, p.sa);
    const cb = across(p.b, p.sb);
    const start = point(p.sa, ca, p.pa);
    const end = point(p.sb, cb, p.pb);
    if (bent.has(i)) return [start, vertical(p.sa) ? { x: end.x, y: start.y } : { x: start.x, y: end.y }, end];
    if (p.pa === p.pb && p.kind !== 'around') return [start, end];
    const r = rank.get(i) ?? 0;
    const [hub, far] = hubOf[i] === 'a' ? [ca, cb] : [cb, ca];
    const dir = Math.sign(far - hub);
    const room = Math.abs(far - hub) - 2 * STUB;
    const m =
      p.kind === 'around'
        ? Math.max(ca, cb) + STUB + r * STEP
        : room <= 0
          ? (ca + cb) / 2
          : hub + dir * (STUB + Math.min(room, r * STEP));
    return [start, point(p.sa, m, p.pa), point(p.sa, m, p.pb), end];
  });
}
