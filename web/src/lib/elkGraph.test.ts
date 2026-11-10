import { describe, expect, it } from 'vitest';
import type { ElkNode } from 'elkjs/lib/elk-api';
import { buildTree } from './expansion';
import { labelWidth, rootOptions, toElkGraph } from './elkGraph';
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
  changes: {},
  edges: [
    { from: 'p1', to: 'p2', count: 3, kinds: { type_ref: 3 }, added: 0, removed: 0 },
    { from: 'p1', to: 'm1', count: 1, kinds: { imports: 1 }, added: 0, removed: 0 },
    { from: 'm1', to: 'p2', count: 1, kinds: { imports: 1 }, added: 0, removed: 0 },
    { from: 'm2', to: 'p1', count: 2, kinds: { extends: 2 }, added: 0, removed: 0 },
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

  it('makes room for the mark of a changed node', () => {
    const marked = buildTree(nodes.map((n) => (n.id === 'p2' ? { ...n, change: 'added' as const } : n)));
    expect(find(toElkGraph(view, marked), 'p2')!.width!).toBeGreaterThan(find(g, 'p2')!.width!);
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

describe('rootOptions', () => {
  const num = (o: Record<string, string>, k: string) => Number(o[k]);

  it('gives the 6-module view about 120 px between layers', () => {
    expect(num(rootOptions(6), 'elk.layered.spacing.nodeNodeBetweenLayers')).toBeGreaterThanOrEqual(110);
  });

  it('keeps the dense spacing for large views', () => {
    const o = rootOptions(400);
    expect(num(o, 'elk.layered.spacing.nodeNodeBetweenLayers')).toBe(28);
    expect(num(o, 'elk.spacing.nodeNode')).toBe(14);
  });

  it('never shrinks spacing when the view gets sparser', () => {
    const keys = ['elk.layered.spacing.nodeNodeBetweenLayers', 'elk.spacing.nodeNode', 'elk.spacing.edgeNode'];
    for (const k of keys) expect(num(rootOptions(10), k)).toBeGreaterThanOrEqual(num(rootOptions(60), k));
  });

  it('gives each edge its own route in sparse views and merges them in dense ones', () => {
    expect(rootOptions(6)['elk.layered.mergeEdges']).toBe('false');
    expect(rootOptions(400)['elk.layered.mergeEdges']).toBe('true');
  });

  it('pads containers more in sparse views', () => {
    const sparse = toElkGraph(view, tree);
    const dense = toElkGraph({ ...view, nodes: view.nodes }, tree, 400);
    const pad = (g: ElkNode) => find(g, 'm1')!.layoutOptions!['elk.padding'];
    expect(pad(sparse)).not.toBe(pad(dense));
    expect(sparse.layoutOptions).toEqual(rootOptions(view.nodes.length));
  });
});

describe('leaf height in sparse views', () => {
  const ids = ['a', 'b', 'c', 'd', 'hub'];
  const t = buildTree(ids.map((id) => ({ id, parent: null, kind: 'module' as const, name: id })));
  const v: View = {
    nodes: ids,
    expandable: [],
    changes: {},
    edges: ['a', 'b', 'c', 'd'].map((f) => ({ from: f, to: 'hub', count: 1, kinds: { imports: 1 }, added: 0, removed: 0 })),
  };

  it('grows with the number of edges on one side so that they do not crowd', () => {
    const g = toElkGraph(v, t);
    expect(find(g, 'hub')!.height!).toBeGreaterThan(find(g, 'a')!.height!);
  });

  it('stays at the plain height where edges are merged', () => {
    for (const n of [30, 400]) {
      const g = toElkGraph(v, t, n);
      expect(find(g, 'hub')!.height).toBe(find(g, 'a')!.height);
    }
  });
});

describe('toElkGraph with a placeholder of the focus cut', () => {
  const focused: View = {
    nodes: ['m1', 'p1', 'rest:m1', 'm2'],
    expandable: ['m1', 'p1'],
    changes: {},
    rest: { m1: 12 },
    edges: [{ from: 'rest:m1', to: 'm2', count: 1, kinds: { imports: 1 }, added: 0, removed: 0 }],
  };
  const g = toElkGraph(focused, tree);

  it('puts the placeholder into its container as a leaf labelled "N more"', () => {
    expect(find(g, 'm1')!.children!.map((c) => c.id)).toEqual(['p1', 'rest:m1']);
    expect(find(g, 'rest:m1')!.width).toBe(labelWidth('12 more', false));
    expect(g.edges!.map((e) => [e.sources[0], e.targets[0]])).toEqual([['rest:m1', 'm2']]);
  });
});
