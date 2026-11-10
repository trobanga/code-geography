import { describe, expect, it } from 'vitest';
import { buildTree } from './expansion';
import { changedFiles } from './files';
import type { ChangedFile, Graph } from './types';

const f = (path: string, module: string | null, change: ChangedFile['change'] = 'modified', test = false): ChangedFile => ({
  path,
  change,
  module,
  test,
});

// m1 { A (modified, in m1/A.java) }, m2 {}, mt (test module)
const graph: Graph = {
  nodes: [
    { id: 'm1', parent: null, kind: 'module', name: 'm1' },
    { id: 'm2', parent: null, kind: 'module', name: 'm2' },
    { id: 'mt', parent: null, kind: 'module', name: 'mt', test: true },
    { id: 'A', parent: 'm1', kind: 'type', name: 'A', file: 'm1/A.java', change: 'modified' },
    { id: 'B', parent: 'm1', kind: 'type', name: 'B', file: 'm1/B.java' },
  ],
  edges: [],
  diff: {
    base: 'b',
    head: 'h',
    files: [
      f('m1/A.java', 'm1'),
      f('m1/B.java', 'm1'),
      f('m1/pom.xml', 'm1', 'added'),
      f('m2/pom.xml', 'm2', 'removed'),
      f('README.md', null),
      f('m1/src/test/ATest.java', 'm1', 'added', true),
      f('mt/x.java', 'mt'),
      f('gone/pom.xml', 'gone'),
    ],
  },
};
const tree = buildTree(graph.nodes);

describe('changedFiles', () => {
  it('groups files by module and marks a file explained when a changed node has its path', () => {
    const r = changedFiles(graph, tree, false);
    expect(r.groups.map((g) => [g.module, g.files.map((x) => [x.path, x.explained])])).toEqual([
      ['m1', [['m1/A.java', true], ['m1/B.java', false], ['m1/pom.xml', false]]],
      ['m2', [['m2/pom.xml', false]]],
      [null, [['README.md', false], ['mt/x.java', false], ['gone/pom.xml', false]]],
    ]);
  });

  it('hides test files and counts them, and shows them with test code on', () => {
    const off = changedFiles(graph, tree, false);
    expect(off.totals).toEqual({ added: 1, removed: 1, modified: 5 });
    expect(off.hidden).toBe(1);
    const on = changedFiles(graph, tree, true);
    expect(on.totals).toEqual({ added: 2, removed: 1, modified: 5 });
    expect(on.hidden).toBe(0);
    expect(on.groups.find((g) => g.module === 'm1')!.files.map((x) => x.path)).toContain('m1/src/test/ATest.java');
    expect(on.groups.map((g) => g.module)).toEqual(['m1', 'm2', 'mt', null]);
  });
});

describe('changedFiles source flag', () => {
  it('marks a file as source when the graph has nodes from files with its extension', () => {
    const r = changedFiles(graph, tree, true);
    const source = Object.fromEntries(r.groups.flatMap((g) => g.files.map((x) => [x.path, x.source])));
    expect(source['m1/B.java']).toBe(true);
    expect(source['mt/x.java']).toBe(true);
    expect(source['m1/pom.xml']).toBe(false);
    expect(source['README.md']).toBe(false);
  });
});

describe('changedFiles relative path', () => {
  it('gives the path inside the module directory of a mod:<dir> module, else the full path', () => {
    const g: Graph = {
      nodes: [{ id: 'mod:a/b', parent: null, kind: 'module', name: 'b' }],
      edges: [],
      diff: { base: 'b', head: 'h', files: [f('a/b/pom.xml', 'mod:a/b'), f('pom.xml', null)] },
    };
    const r = changedFiles(g, buildTree(g.nodes), false);
    expect(r.groups.flatMap((x) => x.files.map((y) => y.rel))).toEqual(['pom.xml', 'pom.xml']);
    expect(r.groups.map((x) => x.module)).toEqual(['mod:a/b', null]);
  });
});
