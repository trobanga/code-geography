// Proof that the C parsers (tree-sitter-java, tree-sitter-rust and tree-sitter-clojure-orchard) link and run inside the WASM module.
// Loads the generated .wasm from disk, as the browser would load it over HTTP.
// @ts-ignore The project has no @types/node; this test is the only file that reads from disk.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { Explorer, diffSources, indexSources, initSync, isInput } from './wasm/cg_wasm.js';
import type { Graph, View } from './types';
import { runJob } from './workerJob';
import { headOf } from './commit';

initSync({ module: readFileSync(new URL('./wasm/cg_wasm_bg.wasm', import.meta.url)) });

const FILES: [string, string][] = [
  ['pom.xml', ''],
  ['src/main/java/a/Foo.java', 'package a;\nimport b.Bar;\npublic class Foo extends Bar {\n  void f() { new Bar().g(); }\n}\n'],
  ['src/main/java/b/Bar.java', 'package b;\npublic class Bar {\n  public void g() {}\n}\n'],
];

function run() {
  const json = indexSources(
    FILES.map(([path]) => path),
    FILES.map(([, text]) => text),
    'repo',
  );
  return JSON.parse(json) as {
    graph: Graph;
    report: { calls: Record<string, number> };
  };
}

describe('the indexer in WASM', () => {
  it('indexes sources given as data', () => {
    const { graph, report } = run();

    const ids = graph.nodes.map((n) => n.id);
    expect(ids).toContain('mod:');
    expect(ids).toContain('type:a.Foo');
    expect(ids).toContain('type:b.Bar');
    expect(ids).toContain('fn:b.Bar#g()');
    expect(graph.nodes.find((n) => n.id === 'mod:')?.name).toBe('repo');
    expect(graph.edges).toContainEqual({ from: 'type:a.Foo', to: 'type:b.Bar', kind: 'extends' });
    expect(graph.edges).toContainEqual({ from: 'fn:a.Foo#f()', to: 'fn:b.Bar#g()', kind: 'calls' });
    expect(report.calls.to_function).toBe(1);
  });

  it('gives a graph that the Explorer cuts', () => {
    const { graph } = run();

    const explorer = new Explorer(JSON.stringify(graph));
    const view = JSON.parse(explorer.view(['mod:', 'pkg::a'], true, true)) as View;
    explorer.free();

    expect(view.nodes).toContain('type:a.Foo');
    expect(view.nodes).toContain('pkg::b');
    expect(view.edges.length).toBeGreaterThan(0);
  });

  it('throws on paths and texts of different length', () => {
    expect(() => indexSources(['A.java'], [], 'repo')).toThrow(/1 paths but 0 texts/);
  });

  it('tells which paths the indexer wants', () => {
    expect(isInput('a/Foo.java')).toBe(true);
    expect(isInput('pom.xml')).toBe(true);
    expect(isInput('src/lib.rs')).toBe(true);
    expect(isInput('Cargo.toml')).toBe(true);
    expect(isInput('deps.edn')).toBe(true);
    expect(isInput('src/a/b.clj')).toBe(true);
    expect(isInput('src/a/b.cljs')).toBe(false);
    expect(isInput('a/target/Foo.java')).toBe(false);
    expect(isInput('a/target/lib.rs')).toBe(false);
    expect(isInput('README.md')).toBe(false);
  });

  it('indexes Rust sources: the grammar and its scanner run inside the module', () => {
    const files: [string, string][] = [
      ['Cargo.toml', '[package]\nname = "my-app"\n'],
      ['src/lib.rs', 'pub mod a;\npub struct Top;\nimpl Top {\n  pub fn run(&self) -> a::A { a::A::new() }\n}\n'],
      ['src/a.rs', 'pub struct A;\nimpl A {\n  pub fn new() -> A { A }\n}\n'],
      ['src/orphan.rs', ''],
    ];
    const json = indexSources(
      files.map(([path]) => path),
      files.map(([, text]) => text),
      'repo',
    );
    const { graph, report } = JSON.parse(json) as {
      graph: Graph;
      report: { calls: Record<string, number>; rust: { unreached_files: string[]; paths: Record<string, number> } };
    };

    const ids = graph.nodes.map((n) => n.id);
    expect(ids).toContain('mod:');
    expect(ids).toContain('pkg::my_app::a');
    expect(ids).toContain('type:my_app::a::A');
    expect(ids).toContain('fn:my_app::Top#run');
    expect(graph.edges).toContainEqual({ from: 'fn:my_app::Top#run', to: 'type:my_app::a::A', kind: 'type_ref' });
    expect(report.rust.unreached_files).toEqual(['src/orphan.rs']);
    expect(report.rust.paths.resolved).toBeGreaterThan(0);
    expect(graph.edges).toContainEqual({ from: 'fn:my_app::Top#run', to: 'fn:my_app::a::A#new', kind: 'calls' });
    expect(report.calls.to_function).toBe(1);
  });

  it('indexes Clojure sources: the grammar runs inside the module', () => {
    const files: [string, string][] = [
      ['deps.edn', '{:aliases {:test {:extra-paths ["test"]}}}'],
      ['src/a.clj', '(ns a (:require [b :as x]))\n(defn f [] (x/g) y)\n(def y 1)\n(defmethod x/m :k [_] (x/g))\n'],
      ['src/b.clj', '(ns b)\n(defn g [])\n(defmulti m identity)\n'],
      ['test/a_test.clj', '(ns a-test)\n(deftest t)\n'],
      ['build.clj', '(ns build)\n'],
    ];
    const json = indexSources(
      files.map(([path]) => path),
      files.map(([, text]) => text),
      'repo',
    );
    const { graph, report } = JSON.parse(json) as {
      graph: Graph;
      report: { clojure: { files_outside_source_dirs: number; value_nodes: number; method_nodes: number } };
    };

    const ids = graph.nodes.map((n) => n.id);
    expect(ids).toContain('mod:');
    expect(ids).toContain('fn:a/f');
    expect(graph.edges).toContainEqual({ from: 'pkg::a', to: 'pkg::b', kind: 'imports' });
    expect(graph.nodes.find((n) => n.id === 'pkg::a-test')?.test).toBe(true);
    expect(report.clojure.files_outside_source_dirs).toBe(1);
    expect(graph.nodes.find((n) => n.id === 'val:a/y')?.kind).toBe('value');
    expect(graph.nodes.find((n) => n.id === 'fn:a/b/m :k')?.kind).toBe('function');
    expect(graph.edges).toContainEqual({ from: 'fn:a/f', to: 'val:a/y', kind: 'value_ref' });
    expect(graph.edges).toContainEqual({ from: 'fn:a/b/m :k', to: 'fn:b/m', kind: 'implements' });
    expect(report.clojure.value_nodes).toBe(1);
    expect(report.clojure.method_nodes).toBe(1);
  });

  it('builds the commit overlay from two source sets', () => {
    const base: [string, string][] = [
      ['pom.xml', ''],
      ['src/main/java/a/Base.java', 'package a;\npublic class Base {}\n'],
      ['src/main/java/a/Gone.java', 'package a;\nclass Gone extends Base {}\n'],
    ];
    const head: [string, string][] = [
      ['pom.xml', ''],
      ['src/main/java/a/Base.java', 'package a;\npublic class Base {}\n'],
      ['src/main/java/a/Sub.java', 'package a;\nclass Sub extends Base {}\n'],
    ];
    const changed: [string, string][] = [
      ['src/main/java/a/Gone.java', 'removed'],
      ['src/main/java/a/Sub.java', 'added'],
      ['README.md', 'modified'],
    ];
    const json = diffSources(
      base.map(([p]) => p),
      base.map(([, t]) => t),
      head.map(([p]) => p),
      head.map(([, t]) => t),
      'repo',
      'v1',
      'v2',
      changed.map(([p]) => p),
      changed.map(([, k]) => k),
    );
    const { graph } = JSON.parse(json) as { graph: Graph; report: { calls: Record<string, number> } };

    expect(graph.nodes.find((n) => n.id === 'type:a.Sub')?.change).toBe('added');
    expect(graph.nodes.find((n) => n.id === 'type:a.Gone')?.change).toBe('removed');
    expect(graph.edges).toContainEqual({ from: 'type:a.Sub', to: 'type:a.Base', kind: 'extends', change: 'added' });
    expect(graph.diff?.base).toBe('v1');
    expect(graph.diff?.head).toBe('v2');
    expect(graph.diff?.files?.map((f) => f.path)).toEqual(changed.map(([p]) => p));
  });

  it('throws on parallel arrays of different length and on an unknown kind', () => {
    expect(() => diffSources(['A.java'], [], [], [], 'repo', 'a', 'b', [], [])).toThrow(/1 base paths but 0 base texts/);
    expect(() => diffSources([], [], [], [], 'repo', 'a', 'b', ['README.md'], ['renamed'])).toThrow(/renamed/);
  });
});

const wasm = { indexSources, diffSources };

describe('runJob', () => {
  it('builds the overlay of two source sets with the changed files', () => {
    const reply = runJob(
      {
        kind: 'diff',
        rootName: 'repo',
        base: { paths: ['pom.xml', 'src/main/java/a/A.java'], texts: ['', 'package a;\nclass A {}\n'] },
        head: { paths: ['pom.xml', 'src/main/java/a/B.java'], texts: ['', 'package a;\nclass B {}\n'] },
        baseName: 'aaaaaaa',
        headName: 'bbbbbbb',
        changed: [
          { path: 'README.md', change: 'modified' },
          { path: 'src/main/java/a/A.java', change: 'removed' },
          { path: 'src/main/java/a/B.java', change: 'added' },
        ],
      },
      wasm,
    );
    if (!reply.ok) throw new Error(reply.error);
    const g = JSON.parse(reply.graphText) as Graph;
    expect(g.nodes.find((n) => n.id === 'type:a.A')?.change).toBe('removed');
    expect(g.nodes.find((n) => n.id === 'type:a.B')?.change).toBe('added');
    expect(g.diff).toMatchObject({ base: 'aaaaaaa', head: 'bbbbbbb' });
    expect(g.diff?.files?.map((f) => f.path)).toEqual(['README.md', 'src/main/java/a/A.java', 'src/main/java/a/B.java']);
    expect(reply.report.calls).toBeDefined();
  });
});

describe('headOf', () => {
  it('gives back the plain graph of the head from the overlay: the same nodes and edges as indexing the head alone', () => {
    const base = {
      paths: ['pom.xml', 'src/main/java/a/A.java', 'src/main/java/a/Gone.java'],
      texts: ['', 'package a;\nclass A { void f() {} }\n', 'package a;\nclass Gone extends A {}\n'],
    };
    const head = {
      paths: ['pom.xml', 'src/main/java/a/A.java', 'src/main/java/a/New.java'],
      texts: ['', 'package a;\nclass A { void f() { g(); } void g() {} }\n', 'package a;\nclass New extends A {}\n'],
    };
    const reply = runJob({ kind: 'diff', rootName: 'r', base, head, baseName: 'b', headName: 'h', changed: [] }, wasm);
    if (!reply.ok) throw new Error(reply.error);
    const plain = JSON.parse(indexSources(head.paths, head.texts, 'r')).graph as Graph;
    const got = headOf(JSON.parse(reply.graphText) as Graph);
    const sorted = (xs: unknown[]) => xs.map((x) => JSON.stringify(x)).sort();
    expect(got.diff).toBeUndefined();
    expect(sorted(got.nodes)).toEqual(sorted(plain.nodes));
    expect(sorted(got.edges)).toEqual(sorted(plain.edges));
  });
});
