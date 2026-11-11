import { describe, expect, it } from 'vitest';
import { boxLines, breakdown, edgesOf } from './detail';
import { buildTree } from './expansion';
import type { GraphNode, View, ViewEdge } from './types';

const view: View = {
  nodes: ['a', 'b', 'c'],
  expandable: [],
  changes: {},
  edges: [
    { from: 'a', to: 'b', count: 2, kinds: { imports: 2 }, added: 0, removed: 0 },
    { from: 'a', to: 'c', count: 9, kinds: { imports: 1, type_ref: 8 }, added: 0, removed: 0 },
    { from: 'c', to: 'a', count: 4, kinds: { extends: 4 }, added: 0, removed: 0 },
  ],
};

describe('edgesOf', () => {
  it('splits edges of a node into outgoing and incoming, each sorted by count descending', () => {
    const { outgoing, incoming } = edgesOf(view, 'a');
    expect(outgoing.map((e) => e.to)).toEqual(['c', 'b']);
    expect(incoming.map((e) => e.from)).toEqual(['c']);
  });
});

describe('breakdown', () => {
  it('lists kinds by count descending', () => {
    expect(breakdown({ imports: 36, type_ref: 80, implements: 11 })).toBe('type_ref 80, imports 36, implements 11');
  });
});

describe('boxLines', () => {
  const node = (id: string, parent: string | null): GraphNode => ({ id, parent, kind: parent ? 'package' : 'module', name: id });
  const tree = buildTree([node('m', null), node('m.a', 'm'), node('m.b', 'm'), node('x', null), node('y', null)]);
  const ve = (from: string, to: string, kinds: ViewEdge['kinds']): ViewEdge => ({
    from,
    to,
    count: Object.values(kinds).reduce((a, b) => a + (b ?? 0), 0),
    kinds,
    added: 0,
    removed: 0,
  });
  // m is open: its children are visible, the view edges end at them.
  const open: View = {
    nodes: ['m', 'm.a', 'm.b', 'x', 'y'],
    expandable: [],
    changes: {},
    edges: [ve('m.a', 'x', { imports: 2 }), ve('m.b', 'x', { calls: 3 }), ve('y', 'm.b', { type_ref: 1 })],
  };
  // The lines after lift: the diagram draws these at the box of m.
  const lines = [ve('m', 'x', { imports: 2, calls: 3 }), ve('y', 'm', { type_ref: 1 })];

  it('lists the lines after lift at an open container, as sums over its contents', () => {
    const r = boxLines(open, tree, lines, 'm');
    expect(r.summed).toBe(true);
    expect(r.outgoing).toEqual([lines[0]]);
    expect(r.incoming).toEqual([lines[1]]);
  });

  it('lists the view edges of a closed box and of a leaf, as before', () => {
    // m is closed: the cut ends the edges at m; the lines of a nested closed box can be lifted away.
    const closed: View = { nodes: ['m', 'x', 'y'], expandable: ['m'], changes: {}, edges: [ve('m', 'x', { imports: 5 }), ve('y', 'm', { type_ref: 1 })] };
    const r = boxLines(closed, tree, [ve('m', 'x', { imports: 2 })], 'm');
    expect(r).toEqual({ ...edgesOf(closed, 'm'), summed: false });
    expect(boxLines(open, tree, lines, 'x')).toEqual({ ...edgesOf(open, 'x'), summed: false });
  });

  it('takes a container as open when only its placeholder shows', () => {
    const folded: View = { nodes: ['m', 'rest:m', 'x'], expandable: [], changes: {}, edges: [ve('rest:m', 'x', { imports: 4 })] };
    const r = boxLines(folded, tree, [ve('m', 'x', { imports: 4 })], 'm');
    expect(r.summed).toBe(true);
    expect(r.outgoing.map((e) => e.to)).toEqual(['x']);
  });
});
