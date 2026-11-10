import { describe, expect, it } from 'vitest';
import { breakdown, edgesOf } from './detail';
import type { View } from './types';

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
