import { describe, expect, it } from 'vitest';
import { activeBoxes, badgeLabel, changeLabel, commitTotals, markedByModule, openChanged, openChangedStep } from './commit';
import { buildTree, pathToOpen } from './expansion';
import type { Graph } from './types';

// m1 { p1 { A (modified) { f (added) }, ATest (test, added) { g (test, added) } } }, m2 { p2 { B (removed) } }
const graph: Graph = {
  diff: { base: 'b0', head: 'h1' },
  nodes: [
    { id: 'm1', parent: null, kind: 'module', name: 'm1' },
    { id: 'm2', parent: null, kind: 'module', name: 'm2' },
    { id: 'p1', parent: 'm1', kind: 'package', name: 'p1' },
    { id: 'p2', parent: 'm2', kind: 'package', name: 'p2' },
    { id: 'A', parent: 'p1', kind: 'type', name: 'A', change: 'modified' },
    { id: 'f', parent: 'A', kind: 'function', name: 'f', change: 'added' },
    { id: 'ATest', parent: 'p1', kind: 'type', name: 'ATest', test: true, change: 'added' },
    { id: 'g', parent: 'ATest', kind: 'function', name: 'g', change: 'added' },
    { id: 'B', parent: 'p2', kind: 'type', name: 'B', change: 'removed' },
  ],
  edges: [
    { from: 'A', to: 'B', kind: 'imports', change: 'removed' },
    { from: 'f', to: 'A', kind: 'type_ref', change: 'added' },
    { from: 'ATest', to: 'A', kind: 'imports', change: 'added' },
    { from: 'g', to: 'f', kind: 'type_ref' },
  ],
};
const tree = buildTree(graph.nodes);

describe('commitTotals', () => {
  it('counts marked nodes and edges outside test code and what test code hides', () => {
    expect(commitTotals(graph, tree, false)).toEqual({
      nodes: { added: 1, removed: 1, modified: 1 },
      edges: { added: 1, removed: 1 },
      hidden: { nodes: 2, edges: 1 },
    });
  });

  it('counts everything with test code on', () => {
    expect(commitTotals(graph, tree, true)).toEqual({
      nodes: { added: 3, removed: 1, modified: 1 },
      edges: { added: 2, removed: 1 },
      hidden: { nodes: 0, edges: 0 },
    });
  });

  it('counts a node below a test container as test code', () => {
    const g: Graph = {
      nodes: [
        { id: 'T', parent: null, kind: 'type', name: 'T', test: true },
        { id: 'h', parent: 'T', kind: 'function', name: 'h', change: 'added' },
      ],
      edges: [],
    };
    expect(commitTotals(g, buildTree(g.nodes), false).hidden.nodes).toBe(1);
  });
});

describe('openChanged', () => {
  it('opens exactly the containers above marked nodes outside test code', () => {
    expect([...openChanged(tree, false)].sort()).toEqual(['A', 'm1', 'm2', 'p1', 'p2']);
  });

  it('also opens containers above marked test code with test code on', () => {
    expect([...openChanged(tree, true)].sort()).toEqual(['A', 'ATest', 'm1', 'm2', 'p1', 'p2']);
  });
});

describe('pathToOpen', () => {
  it('lists the containers above a node top down, without the node', () => {
    expect(pathToOpen(tree, 'f')).toEqual(['m1', 'p1', 'A']);
    expect(pathToOpen(tree, 'm1')).toEqual([]);
  });
});

describe('markedByModule', () => {
  it('lists marked nodes outside test code grouped by module, in tree order', () => {
    expect(markedByModule(tree, false)).toEqual([
      { module: 'm1', nodes: ['A', 'f'] },
      { module: 'm2', nodes: ['B'] },
    ]);
  });

  it('lists only marked nodes strictly below a given node', () => {
    expect(markedByModule(tree, true, 'p1')).toEqual([{ module: 'm1', nodes: ['A', 'f', 'ATest', 'g'] }]);
    expect(markedByModule(tree, true, 'A')).toEqual([{ module: 'm1', nodes: ['f'] }]);
    expect(markedByModule(tree, false, 'ATest')).toEqual([]);
  });
});

describe('changeLabel', () => {
  const ve = (added: number, removed: number) => ({ from: 'a', to: 'b', count: 3, kinds: {}, added, removed });
  it('shows added and removed counts, or nothing for an unchanged edge', () => {
    expect(changeLabel(ve(2, 0))).toBe('+2');
    expect(changeLabel(ve(0, 1))).toBe('−1');
    expect(changeLabel(ve(2, 1))).toBe('+2 −1');
    expect(changeLabel(ve(0, 0))).toBe('');
  });
});

describe('badgeLabel', () => {
  it('shows the marks below a node, without its own mark', () => {
    expect(badgeLabel({ added: 3, removed: 0, modified: 2 })).toBe('+3 ~2');
    expect(badgeLabel({ added: 1, removed: 1, modified: 1 }, 'modified')).toBe('+1 −1');
    expect(badgeLabel({ added: 1, removed: 0, modified: 0 }, 'added')).toBe('');
  });
});

describe('openChangedStep', () => {
  it('first opens down to the changed types and leaves the types closed', () => {
    expect([...openChangedStep(tree, false, new Set())].sort()).toEqual(['m1', 'm2', 'p1', 'p2']);
  });
});

describe('openChangedStep, second step', () => {
  it('opens the changed types also when the first step is already shown', () => {
    const first = openChangedStep(tree, false, new Set());
    expect([...openChangedStep(tree, false, first)].sort()).toEqual(['A', 'm1', 'm2', 'p1', 'p2']);
  });

  it('starts again at the types from any other set', () => {
    expect([...openChangedStep(tree, false, new Set(['m1']))].sort()).toEqual(['m1', 'm2', 'p1', 'p2']);
  });
});

describe('activeBoxes', () => {
  const tree = buildTree(graph.nodes);
  const count = { added: 0, removed: 0, modified: 1 };

  it('holds the boxes with a change at or below them, and the ends of the lines with their containers', () => {
    const changes = { m1: count, p1: count };
    const lines = [{ from: 'A', to: 'B' }];
    expect([...activeBoxes(tree, changes, new Map(), lines)].sort()).toEqual(['A', 'B', 'm1', 'm2', 'p1', 'p2']);
  });
});
