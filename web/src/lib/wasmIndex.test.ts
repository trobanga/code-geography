// Proof that the C parsers (tree-sitter-java and tree-sitter-rust) link and run inside the WASM module.
// Loads the generated .wasm from disk, as the browser would load it over HTTP.
// @ts-ignore The project has no @types/node; this test is the only file that reads from disk.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { Explorer, indexSources, initSync, isInput } from './wasm/cg_wasm.js';
import type { Graph, View } from './types';

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
});
