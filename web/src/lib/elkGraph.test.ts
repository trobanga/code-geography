import { describe, expect, it } from 'vitest';
import type { ElkNode } from 'elkjs/lib/elk-api';
import { buildTree } from './expansion';
import { toElkGraph } from './elkGraph';
import type { GraphNode, View } from './types';

const nodes: GraphNode[] = [
  { id: 'm1', parent: null, kind: 'module', name: 'm1' },
  { id: 'm2', parent: null, kind: 'module', name: 'a-much-longer-module-name' },
  { id: 'p1', parent: 'm1', kind: 'package', name: 'p1' },
  { id: 'p2', parent: 'm1', kind: 'package', name: 'p2' },
  { id: 't1', parent: 'p1', kind: 'type', name: 'T1' },
];
const tree = buildTree(nodes);

const view: View = {
  nodes: ['m1', 'p1', 'p2', 'm2'],
  expandable: ['m1', 'p1'],
  edges: [
    { from: 'p1', to: 'p2', count: 3, kinds: { type_ref: 3 } },
    { from: 'p1', to: 'm1', count: 1, kinds: { imports: 1 } },
    { from: 'm1', to: 'p2', count: 1, kinds: { imports: 1 } },
    { from: 'm2', to: 'p1', count: 2, kinds: { extends: 2 } },
  ],
};

const find = (n: ElkNode, id: string): ElkNode | undefined =>
  n.id === id ? n : (n.children ?? []).map((c) => find(c, id)).find(Boolean);

describe('toElkGraph', () => {
  const g = toElkGraph(view, tree);

  it('nests visible nodes under their parent', () => {
    expect(g.children!.map((c) => c.id)).toEqual(['m1', 'm2']);
    expect(find(g, 'm1')!.children!.map((c) => c.id)).toEqual(['p1', 'p2']);
    expect(find(g, 'p1')!.children ?? []).toEqual([]);
  });

  it('gives leaves a size that grows with label length', () => {
    const short = find(g, 'p2')!;
    const long = find(g, 'm2')!;
    expect(short.height).toBeGreaterThan(0);
    expect(long.width!).toBeGreaterThan(short.width!);
  });

  it('gives an open container a minimum width that fits its label', () => {
    const m1 = find(g, 'm1')!;
    expect(m1.width).toBeUndefined();
    expect(m1.layoutOptions!['elk.nodeSize.minimum']).toMatch(/^\(\d+(\.\d+)?,\s*\d+(\.\d+)?\)$/);
  });

  it('omits edges between a node and its ancestor and keeps the others', () => {
    const pairs = g.edges!.map((e) => `${e.sources[0]}->${e.targets[0]}`);
    expect(pairs).toEqual(['p1->p2', 'm2->p1']);
    expect(new Set(g.edges!.map((e) => e.id)).size).toBe(2);
  });
});
