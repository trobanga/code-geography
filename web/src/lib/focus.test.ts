import { describe, expect, it } from 'vitest';
import { buildTree } from './expansion';
import { background, focusActive, focusOn, labelOf, NO_FOCUS } from './focus';
import type { GraphNode } from './types';

const nodes: GraphNode[] = [
  { id: 'm1', parent: null, kind: 'module', name: 'm1' },
  { id: 'p1', parent: 'm1', kind: 'package', name: 'p1' },
  { id: 't1', parent: 'p1', kind: 'type', name: 'T1' },
];
const tree = buildTree(nodes);

describe('focusActive', () => {
  it('is on with explicit nodes or with changes, and off otherwise', () => {
    expect(focusActive(NO_FOCUS)).toBe(false);
    expect(focusActive({ ...NO_FOCUS, neighbors: true, unfolded: ['m1'] })).toBe(false);
    expect(focusActive({ ...NO_FOCUS, changes: true })).toBe(true);
    expect(focusActive({ ...NO_FOCUS, nodes: ['t1'] })).toBe(true);
  });
});

describe('focusOn', () => {
  it('focuses on one node with its neighbors and opens only the path to it', () => {
    const r = focusOn(tree, 't1');
    expect(r.focus).toEqual({ nodes: ['t1'], changes: false, neighbors: true, unfolded: [] });
    expect([...r.expanded].sort()).toEqual(['m1', 'p1']);
  });
});

describe('labelOf', () => {
  it('labels a placeholder with the number of folded children and other nodes with the tree label', () => {
    expect(labelOf(tree, { rest: { p1: 7 } }, 'rest:p1')).toBe('7 more');
    expect(labelOf(tree, {}, 't1')).toBe('T1');
  });
});

describe('background', () => {
  it('is true for an edge with a placeholder at either end', () => {
    expect(background(tree, { from: 'rest:p1', to: 't1' })).toBe(true);
    expect(background(tree, { from: 't1', to: 'rest:m1' })).toBe(true);
    expect(background(tree, { from: 't1', to: 'p1' })).toBe(false);
  });
});
