import { describe, expect, it } from 'vitest';
import ELK from 'elkjs/lib/elk.bundled.js';
import type { ElkNode } from 'elkjs/lib/elk-api';
import { buildTree } from './expansion';
import { labelWidth, rootOptions, toElkGraph } from './elkGraph';
import { labelOf } from './focus';
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

  it('lays out the hierarchy as one graph to the right while an edge crosses a container border', () => {
    expect(g.layoutOptions!['elk.hierarchyHandling']).toBe('INCLUDE_CHILDREN');
    expect(find(g, 'm1')!.layoutOptions!['elk.direction']).toBeUndefined();
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

  it('puts the placeholder into its container as a leaf labelled with its folded kinds', () => {
    expect(find(g, 'm1')!.children!.map((c) => c.id)).toEqual(['p1', 'rest:m1']);
    expect(find(g, 'rest:m1')!.width).toBe(labelWidth(labelOf(tree, focused, 'rest:m1'), false));
    expect(g.edges!.map((e) => [e.sources[0], e.targets[0]])).toEqual([['rest:m1', 'm2']]);
  });
});

describe('toElkGraph when every edge joins two siblings, as after lifting', () => {
  const sib: View = {
    nodes: ['m1', 'p1', 'p2', 't1', 'm2'],
    expandable: [],
    changes: {},
    edges: [
      { from: 'p1', to: 'p2', count: 1, kinds: { imports: 1 }, added: 0, removed: 0 },
      { from: 'm2', to: 'm1', count: 1, kinds: { imports: 1 }, added: 0, removed: 0 },
    ],
  };
  const g = toElkGraph(sib, tree);

  it('lays out each container on its own, alternating the direction by depth', () => {
    expect(g.layoutOptions!['elk.hierarchyHandling']).toBe('SEPARATE_CHILDREN');
    expect(g.layoutOptions!['elk.direction']).toBe('RIGHT');
    expect(find(g, 'm1')!.layoutOptions!['elk.direction']).toBe('DOWN');
    expect(find(g, 'p1')!.layoutOptions!['elk.direction']).toBe('RIGHT');
    expect(find(g, 'm1')!.layoutOptions!['elk.hierarchyHandling']).toBe('SEPARATE_CHILDREN');
  });

  it('gives each container the routing of the root, which it does not inherit, and the dense spacing', () => {
    const o = find(g, 'm1')!.layoutOptions!;
    for (const k of ['elk.algorithm', 'elk.edgeRouting', 'elk.layered.mergeEdges']) expect(o[k]).toBe(g.layoutOptions![k]);
    const dense = rootOptions(1000);
    for (const k of ['elk.spacing.nodeNode', 'elk.layered.spacing.nodeNodeBetweenLayers', 'elk.spacing.edgeNode', 'elk.spacing.edgeEdge'])
      expect(o[k]).toBe(dense[k]);
  });
});

describe('the ELK layout of a container whose children are leaves', () => {
  // A type with many functions that all call one helper: the callers form one wide layer.
  const fns = Array.from({ length: 30 }, (_, i) => `f${i}`);
  const fanTree = buildTree([
    { id: 'm', parent: null, kind: 'module', name: 'm' },
    { id: 'p', parent: 'm', kind: 'package', name: 'p' },
    { id: 't', parent: 'p', kind: 'type', name: 'SomeTestClass' },
    { id: 'h', parent: 't', kind: 'function', name: 'helper' },
    ...fns.map((f) => ({ id: f, parent: 't', kind: 'function' as const, name: `${f}RejectsAnInvalidPolicy` })),
  ]);
  const edge = (from: string, to: string) => ({ from, to, count: 1, kinds: { calls: 1 }, added: 0, removed: 0 });
  const box = async (v: View, t: typeof fanTree, id: string) => {
    const laid = await new ELK().layout(toElkGraph(v, t));
    const n = find(laid, id)!;
    return n.width! / n.height!;
  };

  it('stacks a wide layer of siblings instead of putting it in one row', async () => {
    const v: View = { nodes: ['m', 'p', 't', 'h', ...fns], expandable: [], changes: {}, edges: fns.map((f) => edge(f, 'h')) };
    expect(await box(v, fanTree, 't')).toBeLessThan(3);
  });

  it('keeps a long chain of siblings out of one row', async () => {
    const chain = fns.slice(0, 12);
    const v: View = { nodes: ['m', 'p', 't', ...chain], expandable: [], changes: {}, edges: chain.slice(1).map((f, i) => edge(chain[i], f)) };
    expect(await box(v, fanTree, 't')).toBeLessThan(3);
  });

  it('stacks a chain of containers that are each wider than high', async () => {
    const types = ['t1', 't2', 't3', 't4'];
    const t = buildTree([
      { id: 'm', parent: null, kind: 'module', name: 'm' },
      { id: 'p', parent: 'm', kind: 'package', name: 'p' },
      ...types.flatMap((ty) => [
        { id: ty, parent: 'p', kind: 'type' as const, name: ty },
        ...fns.slice(0, 8).map((f) => ({ id: `${ty}.${f}`, parent: ty, kind: 'function' as const, name: `${f}RejectsAnInvalidPolicy` })),
      ]),
    ]);
    const v: View = {
      nodes: ['m', 'p', ...types, ...types.flatMap((ty) => fns.slice(0, 8).map((f) => `${ty}.${f}`))],
      expandable: [],
      changes: {},
      edges: [
        ...types.slice(1).map((ty, i) => edge(types[i], ty)),
        ...types.flatMap((ty) => fns.slice(1, 8).map((f) => edge(`${ty}.${f}`, `${ty}.f0`))),
      ],
    };
    expect(await box(v, t, 'p')).toBeLessThan(3);
  });
});

describe('toElkGraph with the changed boxes together', () => {
  const ns: GraphNode[] = [
    { id: 'm', parent: null, kind: 'module', name: 'm' },
    { id: 'n', parent: null, kind: 'module', name: 'n' },
    ...['a', 'b', 'c', 'd'].map((id): GraphNode => ({ id, parent: 'm', kind: 'package', name: id })),
  ];
  const t = buildTree(ns);
  const ids = (n: ElkNode | undefined) => (n?.children ?? []).map((c) => c.id);
  const line = (from: string, to: string) => ({ from, to, count: 1, kinds: { imports: 1 }, added: 1, removed: 0 });
  const v: View = { nodes: ['m', 'n', 'a', 'b', 'c', 'd'], expandable: [], changes: {}, edges: [line('a', 'b')] };

  it('puts the active children of a container into one box and the others into a second box', () => {
    const g = toElkGraph(v, t, undefined, undefined, new Set(['m', 'a', 'b']));
    expect(ids(g)).toEqual(['layout:act:', 'layout:idle:']);
    expect(ids(find(g, 'layout:act:'))).toEqual(['m']);
    expect(ids(find(g, 'layout:idle:'))).toEqual(['n']);
    expect(ids(find(g, 'm'))).toEqual(['layout:act:m', 'layout:idle:m']);
    expect(ids(find(g, 'layout:act:m'))).toEqual(['a', 'b']);
    expect(ids(find(g, 'layout:idle:m'))).toEqual(['c', 'd']);
  });
  // A line that crosses a container border makes ELK lay out the hierarchy as one graph. There the
  // children without a line all go into the first layer: one long column.
  describe('with a line that crosses a container border', () => {
    const many = (p: string) => Array.from({ length: 12 }, (_, i) => `${p}${i}`);
    const pk = (id: string, parent: string): GraphNode => ({ id, parent, kind: 'package', name: id });
    const ns2: GraphNode[] = [
      { id: 'm', parent: null, kind: 'module', name: 'm' },
      { id: 'n', parent: null, kind: 'module', name: 'n' },
      pk('a', 'm'),
      pk('x', 'n'),
      pk('q', 'm'),
      ...many('idle').map((id) => pk(id, 'm')),
      ...many('deep').map((id) => pk(id, 'q')),
      ...many('loose').map((id) => pk(id, 'm')),
    ];
    const t2 = buildTree(ns2);
    const v2: View = { nodes: ns2.map((x) => x.id), expandable: [], changes: {}, edges: [line('a', 'x')] };
    const act = new Set(['m', 'n', 'a', 'x', ...many('loose')]);
    const rows = async (of: string[]) => {
      const res = (await new ELK().layout(toElkGraph(v2, t2, undefined, undefined, act))) as ElkNode;
      return new Set(of.map((id) => Math.round(find(res, id)!.y!))).size;
    };

    it('does not put the idle children into one column', async () => {
      expect(await rows(many('idle'))).toBeLessThan(12);
    });

    it('does not put the children of an idle container into one column', async () => {
      expect(await rows(many('deep'))).toBeLessThan(12);
    });

    it('does not put the active children without a line into one column', async () => {
      expect(await rows(many('loose'))).toBeLessThan(12);
    });
  });
});
