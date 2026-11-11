import { describe, expect, it } from 'vitest';
import { DEFAULT_EDGE_OPTIONS, edgePipeline, filterEdges, IMPLIED_AUTO_FROM, liftEdges, linesText, painted } from './edgeOptions';
import { buildTree } from './expansion';
import type { GraphNode, ViewEdge } from './types';

const e = (from: string, to: string, kinds: ViewEdge['kinds'], added = 0, removed = 0): ViewEdge => ({
  from,
  to,
  count: Object.values(kinds).reduce((a, b) => a + (b ?? 0), 0),
  kinds,
  added,
  removed,
});

describe('filterEdges (O2)', () => {
  const all = { extends: true, implements: true, imports: true, type_ref: true, calls: true, value_ref: true };

  it('keeps every edge with all kinds on', () => {
    const edges = [e('a', 'b', { imports: 1 }), e('b', 'c', { type_ref: 2, extends: 1 })];
    expect(filterEdges(edges, all)).toEqual(edges);
  });

  it('counts only enabled kinds and drops edges that sum to 0', () => {
    const edges = [e('a', 'b', { imports: 4 }), e('b', 'c', { type_ref: 2, imports: 3 })];
    const out = filterEdges(edges, { ...all, imports: false });
    expect(out).toEqual([{ from: 'b', to: 'c', count: 2, kinds: { type_ref: 2 }, added: 0, removed: 0 }]);
  });

  it('switches value_ref like any other kind', () => {
    const edges = [e('f', 'v', { value_ref: 2 }), e('g', 'v', { value_ref: 1, calls: 1 })];
    expect(filterEdges(edges, { ...all, value_ref: false })).toEqual([{ from: 'g', to: 'v', count: 1, kinds: { calls: 1 }, added: 0, removed: 0 }]);
  });

  it('applies the kind filter to changed edges and keeps their marks within the new count', () => {
    const edges = [e('a', 'b', { imports: 1 }, 1, 0), e('c', 'd', { imports: 2, type_ref: 1 }, 3, 0)];
    expect(filterEdges(edges, { ...all, imports: false })).toEqual([
      { from: 'c', to: 'd', count: 1, kinds: { type_ref: 1 }, added: 1, removed: 0 },
    ]);
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
    expect(r.edges).toEqual([{ from: 'm1', to: 'm2', count: 2, kinds: { type_ref: 2 }, added: 0, removed: 0 }]);
  });

  it('drops a placeholder edge that would be lifted, keeps one to a sibling, and counts the dropped', () => {
    const r = liftEdges(
      [e('rest:p1', 't4', { type_ref: 2 }), e('t4', 'rest:p1', { imports: 1 }), e('rest:p1', 't1', { imports: 1 }), e('t1', 't4', { imports: 1 })],
      tree,
    );
    expect(r.edges.map(pair).sort()).toEqual(['m1>m2', 'rest:p1>t1']);
    expect(r.edges.find((x) => x.from === 'm1')!.count).toBe(1);
    expect(r.dropped).toBe(2);
    expect(r.badges.get('t4')).toEqual({ out: 0, in: 1 });
    expect(r.badges.has('rest:p1')).toBe(false);
    expect(r.lifted.map(pair)).toEqual(['t1>t4']);
  });

  it('lifts a placeholder edge like one of a child of its container when no focus is active', () => {
    const r = liftEdges([e('rest:p1', 't4', { type_ref: 2 }), e('t1', 't4', { imports: 1 })], tree, false);
    expect(r.edges).toEqual([{ from: 'm1', to: 'm2', count: 3, kinds: { type_ref: 2, imports: 1 }, added: 0, removed: 0 }]);
    expect(r.dropped).toBe(0);
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
    expect(r.edges).toEqual([{ from: 'm1', to: 'm2', count: 4, kinds: { type_ref: 3, imports: 1 }, added: 0, removed: 0 }]);
  });

  it('adds up added and removed counts when it merges edges', () => {
    const r = liftEdges([e('t1', 't4', { type_ref: 2 }, 2, 0), e('t2', 'p3', { imports: 3 }, 1, 2)], tree);
    expect(r.edges).toEqual([
      { from: 'm1', to: 'm2', count: 5, kinds: { type_ref: 2, imports: 3 }, added: 3, removed: 2 },
    ]);
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

  it('passes the drawable edges through with lift off and counts them', () => {
    const r = edgePipeline(edges, tree, { ...DEFAULT_EDGE_OPTIONS, lift: false });
    expect(pairs(r.edges)).toBe('a>m2 b>m2 m2>m3 m1>m3');
    expect(r.total).toBe(4);
  });

  it('lifts by default', () => {
    const r = edgePipeline(edges, tree, DEFAULT_EDGE_OPTIONS);
    expect(pairs(r.edges)).toBe('m1>m2 m2>m3 m1>m3');
    expect(r.badges.get('a')).toEqual({ out: 1, in: 0 });
  });

  it('with hide implied on, keeps the implied lines out of the drawn edges', () => {
    const r = edgePipeline(edges, tree, { ...DEFAULT_EDGE_OPTIONS, implied: true });
    expect(pairs(r.edges)).toBe('m1>m2 m2>m3');
    expect(pairs(r.implied)).toBe('m1>m3');
    expect(r.hideImplied).toBe(true);
  });

  it('in auto, hides implied lines only above IMPLIED_AUTO_FROM lines', () => {
    const chain = (n: number) => {
      const ns: GraphNode[] = Array.from({ length: n }, (_, i) => ({ id: `n${i}`, parent: null, kind: 'module', name: `n${i}` }));
      const es = ns.slice(1).map((x, i) => e(`n${i}`, x.id, { imports: 1 }));
      return { tree: buildTree(ns), es };
    };
    // A chain of k nodes has k - 1 lines; one chord makes k lines.
    const at = (lines: number) => {
      const c = chain(lines);
      return edgePipeline([...c.es, e('n0', 'n2', { imports: 1 })], c.tree, DEFAULT_EDGE_OPTIONS);
    };
    expect(DEFAULT_EDGE_OPTIONS.implied).toBe(null);
    expect(at(IMPLIED_AUTO_FROM).hideImplied).toBe(false);
    expect(at(IMPLIED_AUTO_FROM).edges.length).toBe(IMPLIED_AUTO_FROM);
    expect(at(IMPLIED_AUTO_FROM + 1).hideImplied).toBe(true);
    expect(at(IMPLIED_AUTO_FROM + 1).implied.map((x) => `${x.from}>${x.to}`)).toEqual(['n0>n2']);
    // An explicit choice wins over auto.
    const c = chain(IMPLIED_AUTO_FROM + 1);
    expect(edgePipeline([...c.es, e('n0', 'n2', { imports: 1 })], c.tree, { ...DEFAULT_EDGE_OPTIONS, implied: false }).hideImplied).toBe(false);
  });

  it('keeps a changed chord and an extends or implements chord', () => {
    const on = { ...DEFAULT_EDGE_OPTIONS, lift: false, implied: true };
    const base = [e('m1', 'm2', { imports: 1 }), e('m2', 'm3', { imports: 1 })];
    expect(edgePipeline([...base, e('m1', 'm3', { imports: 2 }, 1, 0)], tree, on).implied).toEqual([]);
    expect(edgePipeline([...base, e('m1', 'm3', { imports: 2 }, 0, 1)], tree, on).implied).toEqual([]);
    expect(edgePipeline([...base, e('m1', 'm3', { imports: 1, extends: 1 })], tree, on).implied).toEqual([]);
    expect(edgePipeline([...base, e('m1', 'm3', { implements: 1 })], tree, on).implied).toEqual([]);
    expect(pairs(edgePipeline([...base, e('m1', 'm3', { calls: 1, type_ref: 2 })], tree, on).implied)).toBe('m1>m3');
  });
});

describe('painted with changes only', () => {
  it('paints only changed edges', () => {
    expect(painted(e('a', 'b', { imports: 1 }), false, null, null, true)).toBe(false);
    expect(painted(e('a', 'b', { imports: 2 }, 0, 1), false, null, null, true)).toBe(true);
    expect(painted(e('a', 'b', { imports: 2 }, 0, 1), true, 'x', null, true)).toBe(false);
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

describe('linesText', () => {
  it('names the hidden background lines only when there are any', () => {
    expect(linesText(20, 55, 0)).toBe('lines: 20 / 55');
    expect(linesText(20, 55, 12)).toBe('lines: 20 / 55, 12 background hidden');
  });
  it('names the lines hidden as implied only when there are any', () => {
    expect(linesText(118, 417, 0, 299)).toBe('lines: 118 / 417, 299 hidden as implied');
    expect(linesText(20, 55, 12, 3)).toBe('lines: 20 / 55, 12 background hidden, 3 hidden as implied');
    expect(linesText(20, 55, 0, 0)).toBe('lines: 20 / 55');
  });
});
