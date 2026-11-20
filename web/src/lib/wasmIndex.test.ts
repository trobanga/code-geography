// Proof that the C parsers (tree-sitter-java, tree-sitter-rust, tree-sitter-clojure-orchard, tree-sitter-elixir and tree-sitter-typescript) link and run inside the WASM module.
// Loads the generated .wasm from disk, as the browser would load it over HTTP.
// @ts-ignore The project has no @types/node; this test is the only file that reads from disk.
import { readFileSync } from 'node:fs';
// @ts-ignore As above: git objects are SHA-1 named zlib streams.
import { createHash } from 'node:crypto';
// @ts-ignore As above.
import { deflateSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import { Explorer, GitRepo, diffSources, indexSources, initSync, isInput, worktreeOverlay } from './wasm/cg_wasm.js';
import type { Graph, GraphEdge, View } from './types';
import { runJob, worktreeOf } from './workerJob';
import { headOf } from './commit';
import { textReader } from './source';

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
    // Each edge has the lines of its references (ADR 0032).
    expect(graph.edges).toContainEqual({ from: 'type:a.Foo', to: 'type:b.Bar', kind: 'extends', sites: [{ lines: [3] }] });
    expect(graph.edges).toContainEqual({ from: 'fn:a.Foo#f()', to: 'fn:b.Bar#g()', kind: 'calls', sites: [{ lines: [4] }] });
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

  it('gives the edges behind the line between two boxes', () => {
    const { graph } = run();

    const explorer = new Explorer(JSON.stringify(graph));
    const there = JSON.parse(explorer.edgesBetween('type:a.Foo', 'type:b.Bar', false, true)) as GraphEdge[];
    const back = JSON.parse(explorer.edgesBetween('type:b.Bar', 'type:a.Foo', false, true)) as GraphEdge[];
    explorer.free();

    expect(there.length).toBeGreaterThan(0);
    expect(there.map((e) => e.from)).toContain('fn:a.Foo#f()');
    expect(there.every((e) => e.from === 'type:a.Foo' || e.from === 'fn:a.Foo#f()')).toBe(true);
    expect(there.find((e) => e.from === 'fn:a.Foo#f()' && e.kind === 'calls')?.sites).toEqual([{ lines: [4] }]);
    expect(back).toEqual([]);
  });

  it('gives nodes whose file and span the texts of the index show (the source view of a local directory)', async () => {
    const { graph } = run();
    const read = textReader({ head: new Map(FILES) });

    const g = await read(graph.nodes.find((n) => n.id === 'fn:b.Bar#g()')!);

    expect(g.file).toBe('src/main/java/b/Bar.java');
    expect(g.lines.map((l) => l.text)).toEqual(['  public void g() {}']);
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
    expect(graph.edges).toContainEqual({ from: 'fn:my_app::Top#run', to: 'type:my_app::a::A', kind: 'type_ref', sites: [{ lines: [4] }] });
    expect(report.rust.unreached_files).toEqual(['src/orphan.rs']);
    expect(report.rust.paths.resolved).toBeGreaterThan(0);
    expect(graph.edges).toContainEqual({ from: 'fn:my_app::Top#run', to: 'fn:my_app::a::A#new', kind: 'calls', sites: [{ lines: [4] }] });
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
    // A namespace is a package without a file: the site names it.
    expect(graph.edges).toContainEqual({ from: 'pkg::a', to: 'pkg::b', kind: 'imports', sites: [{ file: 'src/a.clj', lines: [1] }] });
    expect(graph.nodes.find((n) => n.id === 'pkg::a-test')?.test).toBe(true);
    expect(report.clojure.files_outside_source_dirs).toBe(1);
    expect(graph.nodes.find((n) => n.id === 'val:a/y')?.kind).toBe('value');
    expect(graph.nodes.find((n) => n.id === 'fn:a/b/m :k')?.kind).toBe('function');
    expect(graph.edges).toContainEqual({ from: 'fn:a/f', to: 'val:a/y', kind: 'value_ref', sites: [{ lines: [2] }] });
    expect(graph.edges).toContainEqual({ from: 'fn:a/b/m :k', to: 'fn:b/m', kind: 'implements', sites: [{ lines: [4] }] });
    expect(report.clojure.value_nodes).toBe(1);
    expect(report.clojure.method_nodes).toBe(1);
  });

  it('indexes Elixir sources: the grammar runs inside the module', () => {
    const files: [string, string][] = [
      ['mix.exs', 'def project, do: [app: :demo]\n'],
      ['lib/a.ex', 'defmodule A do\n  alias B\n  def f(x), do: x\nend\n'],
      ['lib/b.ex', 'defmodule B do\nend\n'],
      ['test/a_test.exs', 'defmodule ATest do\nend\n'],
      ['src/c.ex', 'defmodule C do\nend\n'],
    ];
    const json = indexSources(
      files.map(([path]) => path),
      files.map(([, text]) => text),
      'repo',
    );
    const { graph, report } = JSON.parse(json) as {
      graph: Graph;
      report: { elixir: { files_outside_source_dirs: number; references: Record<string, { resolved: number }> } };
    };

    const ids = graph.nodes.map((n) => n.id);
    expect(ids).toContain('mod:');
    expect(graph.nodes.find((n) => n.id === 'mod:')?.name).toBe('demo');
    expect(ids).toContain('fn:A.f/1');
    // A module is a package without a file: the site names it.
    expect(graph.edges).toContainEqual({ from: 'pkg::A', to: 'pkg::B', kind: 'imports', sites: [{ file: 'lib/a.ex', lines: [2] }] });
    expect(graph.nodes.find((n) => n.id === 'pkg::ATest')?.test).toBe(true);
    // The flag is left out of the JSON when it is false.
    expect(graph.nodes.find((n) => n.id === 'pkg::A')?.test).toBeFalsy();
    expect(ids).not.toContain('pkg::C');
    expect(report.elixir.files_outside_source_dirs).toBe(1);
    expect(report.elixir.references.alias.resolved).toBe(1);
  });

  it('indexes TypeScript sources: the grammar runs inside the module', () => {
    expect(isInput('a.ts')).toBe(true);
    expect(isInput('a.tsx')).toBe(true);
    expect(isInput('package.json')).toBe(true);
    expect(isInput('tsconfig.json')).toBe(true);
    expect(isInput('a.js')).toBe(false);
    const files: [string, string][] = [
      ['package.json', '{"name":"app"}'],
      ['src/a.ts', "import { B } from './b';\nexport class A extends B {}\n"],
      ['src/b.ts', 'export class B {}\n'],
      ['src/b.test.ts', 'export class T {}\n'],
    ];
    const json = indexSources(
      files.map(([path]) => path),
      files.map(([, text]) => text),
      'repo',
    );
    const { graph, report } = JSON.parse(json) as {
      graph: Graph;
      report: { typescript: { imports: { resolved: number } } };
    };

    const ids = graph.nodes.map((n) => n.id);
    expect(ids).toEqual(expect.arrayContaining(['mod:', 'pkg::/src/a', 'pkg::/src/b', 'type::/src/a#A', 'type::/src/b#B']));
    const kinds = (from: string, to: string) => graph.edges.filter((e) => e.from === from && e.to === to).map((e) => e.kind);
    expect(kinds('pkg::/src/a', 'type::/src/b#B')).toEqual(['imports']);
    expect(kinds('type::/src/a#A', 'type::/src/b#B')).toEqual(['extends']);
    expect(report.typescript.imports.resolved).toBe(1);
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
    expect(graph.edges).toContainEqual({ from: 'type:a.Sub', to: 'type:a.Base', kind: 'extends', change: 'added', sites: [{ lines: [2] }] });
    // A removed edge has the lines of the base.
    expect(graph.edges).toContainEqual({ from: 'type:a.Gone', to: 'type:a.Base', kind: 'extends', change: 'removed', sites: [{ lines: [2] }] });
    expect(graph.diff?.base).toBe('v1');
    expect(graph.diff?.head).toBe('v2');
    expect(graph.diff?.files?.map((f) => f.path)).toEqual(changed.map(([p]) => p));
  });

  it('throws on parallel arrays of different length and on an unknown kind', () => {
    expect(() => diffSources(['A.java'], [], [], [], 'repo', 'a', 'b', [], [])).toThrow(/1 base paths but 0 base texts/);
    expect(() => diffSources([], [], [], [], 'repo', 'a', 'b', ['README.md'], ['renamed'])).toThrow(/renamed/);
  });
});

const wasm = { indexSources, diffSources, worktreeOverlay: worktreeOf(GitRepo, worktreeOverlay) };

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

/** A loose object as git writes it: zlib around `<kind> <size>\0<content>`, named by the SHA-1 of the uncompressed bytes. */
function loose(kind: string, content: Uint8Array): { id: string; bytes: Uint8Array } {
  const raw = new Uint8Array([...new TextEncoder().encode(`${kind} ${content.length}\0`), ...content]);
  return { id: createHash('sha1').update(raw).digest('hex'), bytes: new Uint8Array(deflateSync(raw)) };
}

/** The loose objects of a commit with the files `files` (path → text); the commit is the first. */
function commitOf(files: [string, string][]) {
  const objects: { id: string; bytes: Uint8Array }[] = [];
  const hexBytes = (hex: string) => new Uint8Array(hex.match(/../g)!.map((h) => parseInt(h, 16)));
  type Dir = Map<string, Dir | string>;
  const root: Dir = new Map();
  for (const [path, text] of files) {
    const parts = path.split('/');
    let at = root;
    for (const d of parts.slice(0, -1)) at = (at.get(d) as Dir | undefined) ?? (at.set(d, new Map()), at.get(d) as Dir);
    at.set(parts[parts.length - 1], text);
  }
  const write = (dir: Dir): string => {
    const entries = [...dir].map(([name, v]) => {
      const o = typeof v === 'string' ? loose('blob', new TextEncoder().encode(v)) : null;
      if (o) objects.push(o);
      return { name, mode: o ? '100644' : '40000', id: o ? o.id : write(v as Dir), dir: !o };
    });
    // git sorts a directory as if its name ended in `/`.
    entries.sort((a, b) => ((a.dir ? a.name + '/' : a.name) < (b.dir ? b.name + '/' : b.name) ? -1 : 1));
    const body = entries.flatMap((e) => [...new TextEncoder().encode(`${e.mode} ${e.name}\0`), ...hexBytes(e.id)]);
    const tree = loose('tree', new Uint8Array(body));
    objects.push(tree);
    return tree.id;
  };
  const tree = write(root);
  const commit = loose('commit', new TextEncoder().encode(`tree ${tree}\nauthor T <t@x> 0 +0000\ncommitter T <t@x> 0 +0000\n\none\n`));
  return [commit, ...objects];
}

describe('runJob of a worktree', () => {
  it('reads HEAD from the loose objects and builds the overlay of the working tree, with the texts at HEAD', () => {
    const objects = commitOf([
      ['pom.xml', ''],
      ['src/main/java/a/A.java', 'package a;\nclass A {}\n'],
      ['src/main/java/a/Gone.java', 'package a;\nclass Gone {}\n'],
    ]);
    const git = { head: 'ref: refs/heads/main\n', refs: [{ name: 'refs/heads/main', text: `${objects[0].id}\n` }], packedRefs: null, packs: [], loose: objects };
    const reply = runJob(
      {
        kind: 'worktree',
        rootName: 'repo',
        paths: ['pom.xml', 'src/main/java/a/A.java', 'src/main/java/a/New.java'],
        texts: ['', 'package a;\nclass A { int x; }\n', 'package a;\nclass New {}\n'],
        git,
      },
      wasm,
    );
    if (!reply.ok) throw new Error(reply.error);
    const g = JSON.parse(reply.graphText) as Graph;
    expect(g.diff).toMatchObject({ base: objects[0].id.slice(0, 7), head: 'working tree' });
    expect(g.diff?.files?.map((f) => [f.path, f.change])).toEqual([
      ['src/main/java/a/A.java', 'modified'],
      ['src/main/java/a/Gone.java', 'removed'],
      ['src/main/java/a/New.java', 'added'],
    ]);
    expect(reply.base?.['src/main/java/a/A.java']).toBe('package a;\nclass A {}\n');
  });

  it('fails with the reason when HEAD names no commit', () => {
    const git = { head: 'ref: refs/heads/main\n', refs: [], packedRefs: null, packs: [], loose: [] };
    const reply = runJob({ kind: 'worktree', rootName: 'repo', paths: [], texts: [], git }, wasm);
    expect(reply).toEqual({ ok: false, error: 'no commit at HEAD: no ref refs/heads/main' });
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
