import { describe, expect, it } from 'vitest';
import { borderLine, edgeRoutes, GROW_PAD, placeNodes } from './drag';
import type { LaidNode } from './layout';

const n = (id: string, parent: string | null, x: number, y: number, w: number, h: number): LaidNode => ({
  id,
  parent,
  x,
  y,
  w,
  h,
  container: false,
  depth: 0,
});

// m (container) holds p (container) which holds t; s is a sibling of p.
const nodes = [n('m', null, 0, 0, 300, 200), n('p', 'm', 10, 30, 120, 80), n('t', 'p', 20, 60, 50, 22), n('s', 'm', 200, 30, 60, 22)];
const at = (r: { nodes: LaidNode[] }, id: string) => r.nodes.find((x) => x.id === id)!;

describe('placeNodes', () => {
  it('leaves the layout alone without offsets', () => {
    const r = placeNodes(nodes, new Map());
    expect(r.nodes).toEqual(nodes);
    expect(r.shift.get('t')).toEqual({ dx: 0, dy: 0 });
  });

  it('moves a box together with everything inside it', () => {
    const r = placeNodes(nodes, new Map([['p', { dx: 15, dy: 5 }]]));
    expect(at(r, 'p')).toMatchObject({ x: 25, y: 35 });
    expect(at(r, 't')).toMatchObject({ x: 35, y: 65 });
    expect(at(r, 's')).toMatchObject({ x: 200, y: 30 });
    expect(r.shift.get('t')).toEqual({ dx: 15, dy: 5 });
  });

  it('adds the offsets of a node and its ancestors', () => {
    const r = placeNodes(nodes, new Map([['p', { dx: 10, dy: 0 }], ['t', { dx: 0, dy: 4 }]]));
    expect(at(r, 't')).toMatchObject({ x: 30, y: 64 });
  });

  it('grows an ancestor so that it still encloses a child moved out of it', () => {
    const r = placeNodes(nodes, new Map([['t', { dx: 400, dy: 300 }]]));
    const t = at(r, 't');
    const p = at(r, 'p');
    const m = at(r, 'm');
    expect(p.x + p.w).toBe(t.x + t.w + GROW_PAD.side);
    expect(p.y + p.h).toBe(t.y + t.h + GROW_PAD.side);
    expect(m.x + m.w).toBe(p.x + p.w + GROW_PAD.side);
    expect(m.y + m.h).toBe(p.y + p.h + GROW_PAD.side);
    expect(p).toMatchObject({ x: 10, y: 30 });
  });

  it('never shrinks a container below its layout size', () => {
    const r = placeNodes(nodes, new Map([['t', { dx: 5, dy: 0 }]]));
    expect(at(r, 'p')).toMatchObject({ x: 10, y: 30, w: 120, h: 80 });
  });

  it('grows a container to the left and top as well', () => {
    const r = placeNodes(nodes, new Map([['t', { dx: -100, dy: -100 }]]));
    const p = at(r, 'p');
    expect(p.x).toBe(20 - 100 - GROW_PAD.side);
    expect(p.y).toBe(60 - 100 - GROW_PAD.top);
    expect(p.x + p.w).toBe(130);
  });
});

describe('borderLine', () => {
  it('joins the borders of two boxes on the line between their centres', () => {
    const [a, b] = borderLine({ x: 0, y: 0, w: 20, h: 10 }, { x: 100, y: 0, w: 20, h: 10 });
    expect(a).toEqual({ x: 20, y: 5 });
    expect(b).toEqual({ x: 100, y: 5 });
  });

  it('leaves through the top or bottom border when the other box is below', () => {
    const [a, b] = borderLine({ x: 0, y: 0, w: 20, h: 10 }, { x: 0, y: 100, w: 20, h: 10 });
    expect(a).toEqual({ x: 10, y: 10 });
    expect(b).toEqual({ x: 10, y: 100 });
  });
});

describe('edgeRoutes', () => {
  const rects = new Map([
    ['a', { x: 0, y: 0, w: 20, h: 20 }],
    ['b', { x: 100, y: 60, w: 20, h: 20 }],
    ['c', { x: 100, y: 0, w: 20, h: 20 }],
  ]);
  const elk = [
    { x: 0, y: 0 },
    { x: 7, y: 0 },
  ];
  const orthogonal = (pts: { x: number; y: number }[]) =>
    pts.slice(1).every((p, i) => p.x === pts[i].x || p.y === pts[i].y);

  it('routes a line with one moved end at right angles, from border to border', () => {
    const shift = new Map([['a', { dx: 3, dy: 0 }]]);
    const [pts] = edgeRoutes([{ points: elk, from: 'a', to: 'b' }], shift, rects);
    expect(orthogonal(pts)).toBe(true);
    expect(pts.length).toBeGreaterThan(2);
    expect(pts[0].x).toBe(20);
    expect(pts[pts.length - 1].x).toBe(100);
  });

  it('keeps the ELK route when neither end moved', () => {
    expect(edgeRoutes([{ points: elk, from: 'a', to: 'b' }], new Map(), rects)[0]).toBe(elk);
  });

  it('translates the route when both ends moved by the same offset', () => {
    const d = { dx: 3, dy: 4 };
    const shift = new Map([['a', d], ['b', d]]);
    expect(edgeRoutes([{ points: elk, from: 'a', to: 'b' }], shift, rects)[0][1]).toEqual({ x: 10, y: 4 });
  });

  it('gives two new routes at one side of a box distinct attach points', () => {
    const shift = new Map([['a', { dx: 3, dy: 0 }]]);
    const lines = [
      { points: elk, from: 'a', to: 'b' },
      { points: elk, from: 'a', to: 'c' },
    ];
    const [toB, toC] = edgeRoutes(lines, shift, rects);
    expect(toB[0].x).toBe(toC[0].x);
    expect(toB[0].y).not.toBe(toC[0].y);
  });
});
