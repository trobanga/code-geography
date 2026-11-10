import { describe, expect, it } from 'vitest';
import { DEFAULT_EDGE_OPTIONS, edgePipeline, filterEdges, liftEdges, painted, reduceTransitive } from './edgeOptions';
import { buildTree } from './expansion';
import type { GraphNode, ViewEdge } from './types';

const e = (from: string, to: string, kinds: ViewEdge['kinds']): ViewEdge => ({
  from,
  to,
  count: Object.values(kinds).reduce((a, b) => a + (b ?? 0), 0),
  kinds,
});

describe('filterEdges (O2)', () => {
  const all = { extends: true, implements: true, imports: true, type_ref: true };

  it('keeps every edge with all kinds on and min 1', () => {
    const edges = [e('a', 'b', { imports: 1 }), e('b', 'c', { type_ref: 2, extends: 1 })];
    expect(filterEdges(edges, all, 1)).toEqual(edges);
  });

  it('counts only enabled kinds and drops edges that sum to 0', () => {
    const edges = [e('a', 'b', { imports: 4 }), e('b', 'c', { type_ref: 2, imports: 3 })];
    const out = filterEdges(edges, { ...all, imports: false }, 1);
    expect(out).toEqual([{ from: 'b', to: 'c', count: 2, kinds: { type_ref: 2 } }]);
  });

  it('drops edges whose effective count is below min', () => {
    const edges = [e('a', 'b', { imports: 2 }), e('b', 'c', { type_ref: 3 })];
    expect(filterEdges(edges, all, 3).map((x) => x.from)).toEqual(['b']);
  });
});

describe('liftEdges (O3)', () => {
  const nodes: GraphNode[] = [
    { id: 'm1', parent: null, kind: 'module', name: 'm1' },
    { id: 'm2', parent: null, kind: 'module', name: 'm2' },
    { id: 'p1', parent: 'm1', kind: 'package', name: 'p1' },
    { id: 'p2', parent: 'm1', kind: 'package', name: 'p2' },
    { id: 'p3', parent: 'm2', kind: 'package', name: 'p3' },
    { id: 't1', parent: 'p1', kind: 'type', name: 'T1' },
    { id: 't2', parent: 'p1', kind: 'type', name: 'T2' },
    { id: 't4', parent: 'p3', kind: 'type', name: 'T4' },
  ];
  const tree = buildTree(nodes);
  const pair = (x: ViewEdge) => `${x.from}>${x.to}`;

  it('lifts an edge between types in different modules to a module edge', () => {
    const r = liftEdges([e('t1', 't4', { type_ref: 2 })], tree);
    expect(r.edges).toEqual([{ from: 'm1', to: 'm2', count: 2, kinds: { type_ref: 2 } }]);
  });

  it('keeps an edge between two types of the same package', () => {
    const r = liftEdges([e('t1', 't2', { extends: 1 })], tree);
    expect(r.edges.map(pair)).toEqual(['t1>t2']);
    expect(r.badges.size).toBe(0);
    expect(r.lifted).toEqual([]);
  });

  it('lifts an edge from inside an open container to a closed sibling container', () => {
    const r = liftEdges([e('t1', 'p2', { imports: 1 })], tree);
    expect(r.edges.map(pair)).toEqual(['p1>p2']);
  });

  it('keeps edges between top-level nodes (virtual root)', () => {
    const r = liftEdges([e('m1', 'm2', { imports: 1 })], tree);
    expect(r.edges.map(pair)).toEqual(['m1>m2']);
  });

  it('merges equal lifted pairs and adds counts and kinds', () => {
    const r = liftEdges([e('t1', 't4', { type_ref: 2 }), e('t2', 'p3', { type_ref: 1, imports: 1 })], tree);
    expect(r.edges).toEqual([{ from: 'm1', to: 'm2', count: 4, kinds: { type_ref: 3, imports: 1 } }]);
  });

  it('gives each end whose edge was lifted away a badge with outgoing and incoming counts', () => {
    const r = liftEdges(
      [e('t1', 't4', { type_ref: 2 }), e('t2', 't1', { type_ref: 1 }), e('p3', 't1', { imports: 3 })],
      tree,
    );
    expect(r.badges.get('t1')).toEqual({ out: 2, in: 3 });
    expect(r.badges.get('t4')).toEqual({ out: 0, in: 2 });
    expect(r.badges.get('p3')).toEqual({ out: 3, in: 0 });
    expect(r.badges.has('t2')).toBe(false);
    expect(r.lifted.map(pair)).toEqual(['t1>t4', 'p3>t1']);
  });

  it('leaves an edge between a node and its ancestor alone', () => {
    const r = liftEdges([e('t1', 'm1', { imports: 1 })], tree);
    expect(r.edges.map(pair)).toEqual(['t1>m1']);
  });
});

describe('reduceTransitive (O5)', () => {
  const one = (pairs: string) => pairs.split(' ').map((p) => e(p[0], p[1], { imports: 1 }));
  const pairs = (es: ViewEdge[]) => es.map((x) => x.from + x.to).join(' ');

  it('hides the shortcut of a chain', () => {
    expect(pairs(reduceTransitive(one('ab bc ac')))).toBe('ab bc');
  });

  it('hides the shortcut of a diamond and keeps both sides', () => {
    expect(pairs(reduceTransitive(one('ab ac bd cd ad')))).toBe('ab ac bd cd');
  });

  it('never hides edges inside a cycle', () => {
    expect(pairs(reduceTransitive(one('ab bc ca ac')))).toBe('ab bc ca ac');
  });

  it('keeps reachability when several members of a cycle reach the same node', () => {
    // a and b form a cycle; both point to c. Hiding both would lose c.
    expect(pairs(reduceTransitive(one('ab ba ac bc')))).toBe('ab ba ac bc');
  });

  it('hides an edge whose target is reached through a cycle', () => {
    expect(pairs(reduceTransitive(one('xa ab ba bc xc')))).toBe('xa ab ba bc');
  });
});

describe('edgePipeline', () => {
  const nodes: GraphNode[] = [
    { id: 'm1', parent: null, kind: 'module', name: 'm1' },
    { id: 'm2', parent: null, kind: 'module', name: 'm2' },
    { id: 'm3', parent: null, kind: 'module', name: 'm3' },
    { id: 'a', parent: 'm1', kind: 'package', name: 'a' },
    { id: 'b', parent: 'm1', kind: 'package', name: 'b' },
  ];
  const tree = buildTree(nodes);
  const edges = [
    e('a', 'm2', { imports: 1 }),
    e('b', 'm2', { imports: 1 }),
    e('m2', 'm3', { type_ref: 5 }),
    e('m1', 'm3', { type_ref: 5 }),
    e('a', 'm1', { imports: 1 }),
  ];
  const pairs = (es: ViewEdge[]) => es.map((x) => `${x.from}>${x.to}`).join(' ');

  it('passes the drawable edges through with all options off and counts them', () => {
    const r = edgePipeline(edges, tree, DEFAULT_EDGE_OPTIONS);
    expect(pairs(r.edges)).toBe('a>m2 b>m2 m2>m3 m1>m3');
    expect(r.total).toBe(4);
  });

  it('applies min before lift, so small real edges do not add up', () => {
    const r = edgePipeline(edges, tree, { ...DEFAULT_EDGE_OPTIONS, min: 2, lift: true });
    expect(pairs(r.edges)).toBe('m2>m3 m1>m3');
  });

  it('reduces after lift', () => {
    const r = edgePipeline(edges, tree, { ...DEFAULT_EDGE_OPTIONS, lift: true, reduce: true });
    expect(pairs(r.edges)).toBe('m1>m2 m2>m3');
    expect(r.badges.get('a')).toEqual({ out: 1, in: 0 });
  });
});

describe('painted (O1)', () => {
  const ab = e('a', 'b', { imports: 1 });
  it('paints everything when the option is off', () => {
    expect(painted(ab, false, null, null)).toBe(true);
  });
  it('paints nothing without selection and hover', () => {
    expect(painted(ab, true, null, null)).toBe(false);
  });
  it('paints edges at the selected or the hovered node', () => {
    expect(painted(ab, true, 'a', null)).toBe(true);
    expect(painted(ab, true, 'x', 'b')).toBe(true);
    expect(painted(ab, true, 'x', 'y')).toBe(false);
  });
});
