import { describe, expect, it } from 'vitest';
import type { Rect } from './drag';
import type { Point } from './layout';
import { orthoRoutes } from './ortho';

const rects = (r: Record<string, Rect>) => new Map(Object.entries(r));

/** Every segment is horizontal or vertical. */
const orthogonal = (pts: Point[]) => pts.slice(1).every((p, i) => p.x === pts[i].x || p.y === pts[i].y);

type Seg = [Point, Point];
const segs = (pts: Point[]): Seg[] => pts.slice(1).map((p, i) => [pts[i], p]);
/** Two axis-parallel segments share a point (cross or overlap). */
function touch([p, q]: Seg, [r, s]: Seg): boolean {
  const lo = (u: number, v: number) => Math.min(u, v);
  const hi = (u: number, v: number) => Math.max(u, v);
  return (
    lo(p.x, q.x) <= hi(r.x, s.x) && lo(r.x, s.x) <= hi(p.x, q.x) && lo(p.y, q.y) <= hi(r.y, s.y) && lo(r.y, s.y) <= hi(p.y, q.y)
  );
}
const inside = (p: Point, r: Rect) => p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
/** Two segments run on one line and share more than a point. */
function overlap([p, q]: Seg, [r, s]: Seg): boolean {
  const lo = (u: number, v: number) => Math.min(u, v);
  const hi = (u: number, v: number) => Math.max(u, v);
  if (p.x === q.x && r.x === s.x && p.x === r.x) return Math.min(hi(p.y, q.y), hi(r.y, s.y)) > Math.max(lo(p.y, q.y), lo(r.y, s.y));
  if (p.y === q.y && r.y === s.y && p.y === r.y) return Math.min(hi(p.x, q.x), hi(r.x, s.x)) > Math.max(lo(p.x, q.x), lo(r.x, s.x));
  return false;
}
const meet = (u: Point[], v: Point[]) => segs(u).some((a) => segs(v).some((b) => touch(a, b)));

describe('orthoRoutes', () => {
  it('leaves the source at its right side and enters the target at its left side', () => {
    const r = rects({ a: { x: 0, y: 0, w: 100, h: 20 }, b: { x: 300, y: 200, w: 80, h: 20 } });
    const [pts] = orthoRoutes([{ from: 'a', to: 'b' }], r);
    expect(orthogonal(pts)).toBe(true);
    expect(pts.length).toBe(4);
    expect(pts[0]).toEqual({ x: 100, y: 10 });
    expect(pts[3]).toEqual({ x: 300, y: 210 });
    expect(pts[1].x).toBeGreaterThan(100);
    expect(pts[1].x).toBeLessThan(300);
  });

  it('draws one straight segment when the attach points are level', () => {
    const r = rects({ a: { x: 0, y: 0, w: 100, h: 20 }, b: { x: 300, y: 0, w: 80, h: 20 } });
    expect(orthoRoutes([{ from: 'a', to: 'b' }], r)[0]).toEqual([
      { x: 100, y: 10 },
      { x: 300, y: 10 },
    ]);
  });

  it('spreads a fan of lines along the side so that no two lines meet', () => {
    // One source, targets above and below at two depths, in scrambled input order.
    const r = rects({
      s: { x: 0, y: 200, w: 100, h: 40 },
      t1: { x: 300, y: 0, w: 80, h: 20 },
      t2: { x: 400, y: 80, w: 80, h: 20 },
      t3: { x: 300, y: 330, w: 80, h: 20 },
      t4: { x: 450, y: 420, w: 80, h: 20 },
      t5: { x: 350, y: 150, w: 80, h: 20 },
    });
    const lines = ['t3', 't1', 't5', 't4', 't2'].map((to) => ({ from: 's', to }));
    const routes = orthoRoutes(lines, r);
    for (const pts of routes) {
      expect(orthogonal(pts)).toBe(true);
      expect(pts[0].x).toBe(100);
    }
    const starts = routes.map((pts) => pts[0].y);
    expect(new Set(starts).size).toBe(5);
    for (const y of starts) expect(y > 200 && y < 240).toBe(true);
    for (let i = 0; i < routes.length; i++)
      for (let j = i + 1; j < routes.length; j++) expect(meet(routes[i], routes[j]), `${lines[i].to} ${lines[j].to}`).toBe(false);
  });

  it('leaves at the left side and enters at the right side when the target is to the left', () => {
    const r = rects({ a: { x: 300, y: 200, w: 80, h: 20 }, b: { x: 0, y: 0, w: 100, h: 20 } });
    const [pts] = orthoRoutes([{ from: 'a', to: 'b' }], r);
    expect(orthogonal(pts)).toBe(true);
    expect(pts[0]).toEqual({ x: 300, y: 210 });
    expect(pts.at(-1)).toEqual({ x: 100, y: 10 });
  });

  it('runs vertically from bottom to top when the boxes overlap in x', () => {
    const r = rects({ a: { x: 0, y: 0, w: 100, h: 20 }, b: { x: 60, y: 200, w: 100, h: 20 }, c: { x: 80, y: -200, w: 40, h: 20 } });
    const [down, up] = orthoRoutes([{ from: 'a', to: 'b' }, { from: 'a', to: 'c' }], r);
    expect(orthogonal(down) && orthogonal(up)).toBe(true);
    expect(down[0].y).toBe(20);
    expect(down.at(-1)!.y).toBe(200);
    expect(up[0].y).toBe(0);
    expect(up.at(-1)!.y).toBe(-180);
  });

  it('runs from the border of the outer box to the inner box when one holds the other', () => {
    const outer = { x: 0, y: 0, w: 400, h: 300 };
    const inner = { x: 150, y: 100, w: 80, h: 20 };
    const r = rects({ o: outer, i: inner });
    const [inward, outward] = orthoRoutes([{ from: 'o', to: 'i' }, { from: 'i', to: 'o' }], r);
    for (const pts of [inward, outward]) {
      expect(orthogonal(pts)).toBe(true);
      expect(pts.every((p) => inside(p, outer))).toBe(true);
      expect(pts.length).toBeGreaterThanOrEqual(2);
    }
    expect(inward[0].x).toBe(0);
    expect(inward.at(-1)!.x).toBe(150);
    expect(outward[0].x).toBe(230);
    expect(outward.at(-1)!.x).toBe(400);
  });

  it('goes around the right sides when the boxes are too close to pass between them', () => {
    const a = { x: 0, y: 0, w: 100, h: 20 };
    const b = { x: 10, y: 30, w: 120, h: 20 };
    const [pts] = orthoRoutes([{ from: 'a', to: 'b' }], rects({ a, b }));
    expect(orthogonal(pts)).toBe(true);
    expect(pts[0].x).toBe(100);
    expect(pts.at(-1)!.x).toBe(130);
    // The last segment comes from outside b.
    expect(pts.at(-2)!.x).toBeGreaterThan(130);
  });

  it('gives each line the same route whatever the input order, and opposite lines apart', () => {
    const r = rects({
      s: { x: 200, y: 200, w: 100, h: 40 },
      a: { x: 500, y: 100, w: 80, h: 20 },
      b: { x: 0, y: 300, w: 80, h: 20 },
      c: { x: 220, y: 0, w: 60, h: 20 },
    });
    const lines = [
      { from: 's', to: 'a' },
      { from: 'a', to: 's' },
      { from: 'b', to: 's' },
      { from: 's', to: 'c' },
      { from: 'c', to: 's' },
    ];
    const key = (l: { from: string; to: string }) => `${l.from}>${l.to}`;
    const byKey = (ls: typeof lines) => {
      const routes = orthoRoutes(ls, r);
      return new Map(ls.map((l, i) => [key(l), routes[i]] as const).sort(([u], [v]) => (u < v ? -1 : 1)));
    };
    const one = byKey(lines);
    expect(byKey([...lines].reverse())).toEqual(one);
    expect(meet(one.get('s>a')!, one.get('a>s')!)).toBe(false);
    expect(meet(one.get('s>c')!, one.get('c>s')!)).toBe(false);
  });

  it('moves lines to the top and bottom sides when a side is too short for them', () => {
    const r: Record<string, Rect> = { hub: { x: 600, y: 300, w: 80, h: 20 } };
    const lines = [];
    for (let i = 0; i < 12; i++) {
      r[`s${i}`] = { x: 50 * (i % 3), y: i * 60, w: 80, h: 20 };
      lines.push({ from: `s${i}`, to: 'hub' });
    }
    const routes = orthoRoutes(lines, rects(r));
    const ends = routes.map((pts) => pts.at(-1)!);
    for (const pts of routes) expect(orthogonal(pts)).toBe(true);
    for (let i = 0; i < ends.length; i++)
      for (let j = i + 1; j < ends.length; j++) {
        expect(Math.hypot(ends[i].x - ends[j].x, ends[i].y - ends[j].y)).toBeGreaterThanOrEqual(4);
        expect(segs(routes[i]).some((u) => segs(routes[j]).some((v) => overlap(u, v)))).toBe(false);
      }
    expect(ends.some((p) => p.y === 300)).toBe(true);
    expect(ends.some((p) => p.y === 320)).toBe(true);
  });
});
