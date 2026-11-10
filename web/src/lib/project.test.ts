import { describe, expect, it } from 'vitest';
import { chooseInputs, inputError, splitRelative, summarize } from './project';

describe('splitRelative', () => {
  it('takes the first segment off as the root name', () => {
    expect(splitRelative('fts-next/clinical-domain/pom.xml')).toEqual({ root: 'fts-next', path: 'clinical-domain/pom.xml' });
  });

  it('turns backslashes into slashes', () => {
    expect(splitRelative('repo\\src\\main\\java\\A.java')).toEqual({ root: 'repo', path: 'src/main/java/A.java' });
  });
});

describe('chooseInputs', () => {
  const isJava = (p: string) => p.endsWith('.java') || p.endsWith('pom.xml');

  it('keeps the files that the predicate wants, with paths below the root', () => {
    const files = ['repo/pom.xml', 'repo/README.md', 'repo/src/A.java'];
    const r = chooseInputs(files, (f) => f, isJava);
    expect(r.root).toBe('repo');
    expect(r.inputs).toEqual([
      { path: 'pom.xml', file: 'repo/pom.xml' },
      { path: 'src/A.java', file: 'repo/src/A.java' },
    ]);
  });
});

describe('summarize', () => {
  const graph = { nodes: [{ id: 'mod:' }, { id: 'pkg::a' }, { id: 'type:a.A' }], edges: [{}, {}] };
  const report = { calls: { to_function: 5, to_type: 2, external: 7, unresolved: 3 } };

  it('counts source files, nodes and edges and splits the calls into resolved and unresolved', () => {
    const s = summarize('repo', ['pom.xml', 'a/A.java', 'a/B.java'], graph, report);
    expect(s).toMatchObject({ name: 'repo', sources: 2, nodes: 3, edges: 2, resolved: 7, unresolved: 3 });
    expect(s.text).toBe('repo: 2 source files, 3 nodes, 2 edges, calls: 7 resolved, 3 unresolved');
  });

  it('counts .rs files as source files too, but not Cargo.toml', () => {
    const s = summarize('repo', ['Cargo.toml', 'src/lib.rs', 'src/a.rs', 'a/B.java'], graph, report);
    expect(s.sources).toBe(3);
  });

  it('gives the four call numbers in the tooltip', () => {
    const s = summarize('repo', [], graph, report);
    expect(s.tooltip).toContain('to a function: 5');
    expect(s.tooltip).toContain('to a type: 2');
    expect(s.tooltip).toContain('external: 7');
    expect(s.tooltip).toContain('unresolved: 3');
  });
});

describe('inputError', () => {
  it('says that only Java and Rust are supported when no input is found', () => {
    expect(inputError({ root: 'docs', inputs: [] })).toBe(
      'no Java or Rust sources found in docs (only Java and Rust are supported now)',
    );
  });

  it('accepts a directory with a Rust source file', () => {
    expect(inputError({ root: 'p', inputs: [{ path: 'src/lib.rs', file: null }] })).toBeNull();
  });
});

describe('inputError with build files only', () => {
  it('counts a directory with pom.xml and Cargo.toml files but no source file as without sources', () => {
    expect(inputError({ root: 'p', inputs: [{ path: 'pom.xml', file: null }, { path: 'Cargo.toml', file: null }] })).toMatch(/no Java or Rust sources found in p/);
  });
});
