import { describe, expect, it } from 'vitest';
import { buildTree } from './expansion';
import type { KindSwitch } from './edgeOptions';
import { memoryCache, type Entry, type Forge, type Repo } from './forge';
import {
  declarations,
  hasLines,
  lineGroups,
  lineSummary,
  overlayLines,
  qualifier,
  readInto,
  sourceReader,
  sourceView,
  spanLines,
  splitLines,
  type SourceLine,
  type SourceState,
} from './source';
import type { GraphEdge, GraphNode } from './types';

const FIVE = 'one\ntwo\nthree\nfour\nfive\n';

describe('splitLines', () => {
  it('starts no further line after a line end at the end of the text', () => {
    expect(splitLines('a\nb\n')).toEqual(['a', 'b']);
  });

  it('ends a line at \\r\\n too', () => {
    expect(splitLines('a\r\nb\r\n\r\nc\r\n')).toEqual(['a', 'b', '', 'c']);
  });

  it('gives no line for an empty text and keeps the last line of a text without a final line end', () => {
    expect(splitLines('')).toEqual([]);
    expect(splitLines('a\nb')).toEqual(['a', 'b']);
    expect(splitLines('\n')).toEqual(['']);
  });
});

describe('spanLines', () => {
  it('gives the lines of the span with their numbers in the file and no marks', () => {
    expect(spanLines(FIVE, { start_line: 2, end_line: 3 })).toEqual([
      { no: 2, text: 'two', mark: null },
      { no: 3, text: 'three', mark: null },
    ]);
  });

  it('gives the lines that exist for a span past the end', () => {
    expect(spanLines(FIVE, { start_line: 4, end_line: 9 })).toEqual([
      { no: 4, text: 'four', mark: null },
      { no: 5, text: 'five', mark: null },
    ]);
  });

  it('starts at the first line for a span that starts before it', () => {
    expect(spanLines(FIVE, { start_line: 0, end_line: 2 })).toEqual([
      { no: 1, text: 'one', mark: null },
      { no: 2, text: 'two', mark: null },
    ]);
  });
});

/** A text of lines `a`, `b`, ... given as the letters. */
const text = (letters: string) => letters.split('').join('\n') + (letters === '' ? '' : '\n');
/** The lines as `no text mark`, with `+` for added, `-` for removed and `=` for none. */
const shown = (lines: SourceLine[]) => lines.map((l) => `${l.no} ${l.text} ${l.mark === 'added' ? '+' : l.mark === 'removed' ? '-' : '='}`);
const span = (start_line: number, end_line: number) => ({ start_line, end_line });

describe('overlayLines at the head', () => {
  it('marks nothing when the commit did not change the file', () => {
    const lines = overlayLines(text('abcd'), text('abcd'), span(2, 3), 'head');

    expect(shown(lines)).toEqual(['2 b =', '3 c =']);
  });

  it('marks a line that the commit added inside the span', () => {
    const lines = overlayLines(text('abcd'), text('abXcd'), span(2, 4), 'head');

    expect(shown(lines)).toEqual(['2 b =', '3 X +', '4 c =']);
  });

  it('shows a line that the commit removed inside the span at its place with its number in the base', () => {
    const lines = overlayLines(text('abcd'), text('acd'), span(1, 3), 'head');

    expect(shown(lines)).toEqual(['1 a =', '2 b -', '2 c =', '3 d =']);
  });

  it('shows the old first line above the new one when the commit replaced the first line', () => {
    const lines = overlayLines(text('abc'), text('Xbc'), span(1, 3), 'head');

    expect(shown(lines)).toEqual(['1 a -', '1 X +', '2 b =', '3 c =']);
  });

  it('shows the old first line also for a span that starts further down', () => {
    const lines = overlayLines(text('abcd'), text('aXcd'), span(2, 3), 'head');

    expect(shown(lines)).toEqual(['2 b -', '2 X +', '3 c =']);
  });

  it('shows the old last line above the new one when the commit replaced the last line', () => {
    const lines = overlayLines(text('abc'), text('abX'), span(1, 3), 'head');

    expect(shown(lines)).toEqual(['1 a =', '2 b =', '3 c -', '3 X +']);
  });

  it('does not show a removed line directly above an unchanged first line', () => {
    const lines = overlayLines(text('abcd'), text('acd'), span(2, 3), 'head');

    expect(shown(lines)).toEqual(['2 c =', '3 d =']);
  });

  it('does not show it either when the span has an added line further down', () => {
    const lines = overlayLines(text('abcd'), text('acXd'), span(2, 3), 'head');

    expect(shown(lines)).toEqual(['2 c =', '3 X +']);
  });

  it('shows no line that the commit changed outside the span, and only shifts the numbers', () => {
    expect(shown(overlayLines(text('abcde'), text('XabcdeZ'), span(2, 4), 'head'))).toEqual(['2 a =', '3 b =', '4 c =']);
    expect(shown(overlayLines(text('Qabcde'), text('abcde'), span(1, 3), 'head'))).toEqual(['1 a =', '2 b =', '3 c =']);
    expect(shown(overlayLines(text('abcde'), text('abcd'), span(1, 4), 'head'))).toEqual(['1 a =', '2 b =', '3 c =', '4 d =']);
  });

  it('marks every line as added when the base has no such file', () => {
    const lines = overlayLines(undefined, text('abc'), span(1, 2), 'head');

    expect(shown(lines)).toEqual(['1 a +', '2 b +']);
  });
});

const gnode = (id: string, parent: string | null, kind: GraphNode['kind'], extra: Partial<GraphNode> = {}): GraphNode => ({
  id,
  parent,
  kind,
  name: id,
  ...extra,
});
/** A node with lines of its own. */
const code = (id: string, parent: string, kind: GraphNode['kind'], extra: Partial<GraphNode> = {}) =>
  gnode(id, parent, kind, { file: `${id}.src`, span: span(1, 3), ...extra });

/** Module `M` with the package `p` (the types `T` with a method, `TT` a test type, the function `g` and the value `v`) and its package `q` with the type `U`. */
const NODES = [
  gnode('M', null, 'module'),
  gnode('p', 'M', 'package'),
  code('T', 'p', 'type'),
  code('T.f', 'T', 'function'),
  code('TT', 'p', 'type', { test: true }),
  code('TT.t', 'TT', 'function', { test: true }),
  code('g', 'p', 'function'),
  gnode('q', 'p', 'package'),
  code('U', 'q', 'type'),
  code('v', 'p', 'value'),
];

describe('declarations', () => {
  it('lists the topmost nodes with lines below a container in tree order and not their members', () => {
    expect(declarations(buildTree(NODES), 'M', false)).toEqual(['T', 'g', 'U', 'v']);
    expect(declarations(buildTree(NODES), 'q', false)).toEqual(['U']);
  });

  it('leaves out test code, and lists it with `tests`', () => {
    expect(declarations(buildTree(NODES), 'p', true)).toEqual(['T', 'TT', 'g', 'U', 'v']);
  });

  it('lists nothing for a leaf', () => {
    expect(declarations(buildTree(NODES), 'g', true)).toEqual([]);
    expect(declarations(buildTree(NODES), 'no such node', true)).toEqual([]);
  });
});

describe('qualifier', () => {
  const tree = buildTree(NODES);

  it('is the label of the parent with a dot, for a node in a type or in a package', () => {
    expect(qualifier(tree, 'T.f')).toBe('T.');
    expect(qualifier(tree, 'U')).toBe('q.');
  });

  it('is empty for a node of a module, for a root and for an id that is no node', () => {
    expect(qualifier(tree, 'p')).toBe('');
    expect(qualifier(tree, 'M')).toBe('');
    expect(qualifier(tree, 'ghost')).toBe('');
  });
});

const ALL_KINDS: KindSwitch = { extends: true, implements: true, imports: true, type_ref: true, calls: true, value_ref: true };
const edge = (from: string, to: string, kind: GraphEdge['kind'], change?: GraphEdge['change']): GraphEdge =>
  change === undefined ? { from, to, kind } : { from, to, kind, change };

describe('lineGroups', () => {
  it('groups the edges by their from declaration and their targets in the order of first appearance', () => {
    const groups = lineGroups(
      [edge('f', 'x', 'calls'), edge('g', 'y', 'calls'), edge('f', 'z', 'calls'), edge('g', 'y', 'calls'), edge('f', 'x', 'calls')],
      ALL_KINDS,
    );

    expect(groups).toEqual([
      {
        from: 'f',
        targets: [
          { id: 'x', kinds: ['calls'], change: null },
          { id: 'z', kinds: ['calls'], change: null },
        ],
      },
      { from: 'g', targets: [{ id: 'y', kinds: ['calls'], change: null }] },
    ]);
  });

  it('gives one target with the kinds in the order of EDGE_KINDS for edges of different kinds to it', () => {
    const groups = lineGroups([edge('f', 'x', 'calls'), edge('f', 'x', 'extends'), edge('f', 'x', 'imports')], ALL_KINDS);

    expect(groups).toEqual([{ from: 'f', targets: [{ id: 'x', kinds: ['extends', 'imports', 'calls'], change: null }] }]);
  });

  it('leaves out the edges of a kind that is switched off, and a group that loses all its targets', () => {
    const groups = lineGroups(
      [edge('f', 'x', 'calls'), edge('f', 'x', 'imports'), edge('g', 'y', 'imports')],
      { ...ALL_KINDS, imports: false },
    );

    expect(groups).toEqual([{ from: 'f', targets: [{ id: 'x', kinds: ['calls'], change: null }] }]);
  });

  it('takes the mark of the edges to a target when all carry the same one', () => {
    const groups = lineGroups(
      [
        edge('f', 'x', 'calls', 'added'),
        edge('f', 'x', 'type_ref', 'added'),
        edge('f', 'y', 'calls', 'added'),
        edge('f', 'y', 'type_ref'),
        edge('f', 'z', 'calls', 'removed'),
        edge('f', 'w', 'calls', 'added'),
        edge('f', 'w', 'type_ref', 'removed'),
      ],
      ALL_KINDS,
    );

    expect(groups[0].targets.map((t) => [t.id, t.change])).toEqual([
      ['x', 'added'],
      ['y', null],
      ['z', 'removed'],
      ['w', null],
    ]);
  });
});

describe('sourceView', () => {
  const tree = buildTree(NODES);
  const edges: Record<string, GraphEdge[]> = {
    'p>q': [edge('g', 'U', 'calls')],
    'q>p': [edge('U', 'T', 'type_ref'), edge('U', 'g', 'calls')],
  };
  const calls: string[] = [];
  const between = (from: string, to: string) => {
    calls.push(`${from}>${to}`);
    return edges[`${from}>${to}`] ?? [];
  };
  const view = (selection: string[], tests = false) => {
    calls.length = 0;
    return sourceView(tree, selection, tests, between);
  };
  const rows = (...ids: string[]) => ids.map((from) => ({ from, targets: [] }));

  it('shows the lines of one box that has lines of its own, and does not ask for edges', () => {
    expect(view(['T'])).toEqual({ kind: 'code', node: 'T' });
    expect(calls).toEqual([]);
  });

  it('lists the declarations of one container without lines of its own as rows without targets, also when there are none', () => {
    expect(view(['q'])).toEqual({ kind: 'list', of: 'q', rows: rows('U') });
    expect(view(['p'], true)).toEqual({ kind: 'list', of: 'p', rows: rows('T', 'TT', 'g', 'U', 'v') });
    expect(view(['M'])).toEqual({ kind: 'list', of: 'M', rows: rows('T', 'g', 'U', 'v') });
    expect(sourceView(buildTree([gnode('e', null, 'module')]), ['e'], false, between)).toEqual({ kind: 'list', of: 'e', rows: [] });
    expect(calls).toEqual([]);
  });

  it('gives the edges of both directions for two boxes, asking for (a, b) and then (b, a)', () => {
    expect(view(['p', 'q'])).toEqual({ kind: 'pair', a: 'p', b: 'q', there: edges['p>q'], back: edges['q>p'] });
    expect(calls).toEqual(['p>q', 'q>p']);
  });

  it('has nothing to show for no selection, three boxes or an id that is no node', () => {
    expect(view([])).toBeNull();
    expect(view(['T', 'g', 'q'])).toBeNull();
    expect(view(['nope'])).toBeNull();
    expect(view(['T', 'nope'])).toBeNull();
    expect(calls).toEqual([]);
  });
});

describe('hasLines', () => {
  it('holds for a node with a file and a span only', () => {
    expect(hasLines(code('T', 'p', 'type'))).toBe(true);
    expect(hasLines(gnode('p', 'M', 'package'))).toBe(false);
    expect(hasLines(gnode('x', 'p', 'type', { file: 'x.src' }))).toBe(false);
    expect(hasLines(gnode('x', 'p', 'type', { span: span(1, 2) }))).toBe(false);
  });
});

const HEAD = 'h'.repeat(40);
const BASE = 'b'.repeat(40);
const REPO = { owner: 'o', name: 'r' };

/**
 * A forge of files in memory, per commit sha. A blob sha is `blob:` and the text, so equal texts
 * share a blob. `calls` records each call. It has only what `sourceReader` uses.
 */
function fakeForge(files: Record<string, Record<string, string>>, failure?: string) {
  const calls: string[] = [];
  const forge = {
    host: 'example.org',
    name: 'example.org',
    async tree(_repo: Repo, sha: string) {
      calls.push(`tree ${sha.slice(0, 1)}`);
      return Object.entries(files[sha] ?? {}).map(([path, text]): Entry => ({ path, blobSha: `blob:${text}` }));
    },
    async text(_repo: Repo, sha: string, path: string) {
      calls.push(`text ${sha.slice(0, 1)} ${path}`);
      if (failure !== undefined) throw new Error(failure);
      return files[sha][path];
    },
  } as unknown as Forge;
  return { forge, calls };
}

function reader(files: Record<string, Record<string, string>>, base: string | null | undefined, cache = memoryCache()) {
  const { forge, calls } = fakeForge(files);
  const read = sourceReader({ forge: () => forge, repo: REPO, cache: async () => cache, head: HEAD, base });
  return { read, calls, cache };
}

describe('sourceReader without an overlay', () => {
  it('gives the path of the file and the lines of the span at the head', async () => {
    const { read } = reader({ [HEAD]: { 'src/A.java': 'one\ntwo\nthree\nfour\n' } }, undefined);

    const source = await read({ file: 'src/A.java', span: span(2, 3) });

    expect(source.file).toBe('src/A.java');
    expect(shown(source.lines)).toEqual(['2 two =', '3 three =']);
  });

  it('takes a text from the cache by its blob sha, asks the forge once for a miss, and then has it in the cache', async () => {
    const files = { [HEAD]: { 'A.java': 'one\ntwo\n', 'B.java': 'three\nfour\n' } };
    const cache = memoryCache();
    await cache.put('blob:one\ntwo\n', 'one\ntwo\n');
    const { read, calls } = reader(files, undefined, cache);

    await read({ file: 'A.java', span: span(1, 2) });
    expect(calls.filter((c) => c.startsWith('text'))).toEqual([]);

    await read({ file: 'B.java', span: span(1, 2) });
    await read({ file: 'B.java', span: span(2, 2) });
    expect(calls.filter((c) => c.startsWith('text'))).toEqual(['text h B.java']);
  });

  it('reads the file list of a commit once, also when the cache keeps nothing', async () => {
    const files = { [HEAD]: { 'A.java': 'one\n', 'B.java': 'two\n' } };
    const inner = memoryCache();
    const forgetting = { ...inner, put: async () => {} };
    const { read, calls } = reader(files, undefined, forgetting);

    await read({ file: 'A.java', span: span(1, 1) });
    await read({ file: 'B.java', span: span(1, 1) });
    await Promise.all([read({ file: 'A.java', span: span(1, 1) }), read({ file: 'B.java', span: span(1, 1) })]);

    expect(calls.filter((c) => c.startsWith('tree'))).toEqual(['tree h']);
  });
});

describe('lineSummary', () => {
  const group = (from: string, ...targets: string[]) => ({ from, targets: targets.map((id) => ({ id, kinds: [], change: null })) });

  it('counts the declarations of a line and their targets, in the singular for one', () => {
    expect(lineSummary([group('f', 'x')])).toBe('1 declaration, 1 target');
    expect(lineSummary([group('f', 'x', 'y'), group('g', 'x')])).toBe('2 declarations, 3 targets');
  });

  it('says that a direction without edges has nothing', () => {
    expect(lineSummary([])).toBe('nothing');
  });
});

describe('readInto', () => {
  const node = { file: 'A.java', span: span(1, 1) };
  const source = { file: 'A.java', lines: [{ no: 1, text: 'x', mark: null }] };
  const settled = () => new Promise((done) => setTimeout(done, 0));

  it('reports that it reads, then the lines', async () => {
    const states: SourceState[] = [];

    readInto(async () => source, node, (s) => states.push(s));

    expect(states).toEqual([{ status: 'reading' }]);
    await settled();
    expect(states).toEqual([{ status: 'reading' }, { status: 'lines', source }]);
  });

  it('reports the message of a failed read', async () => {
    const states: SourceState[] = [];

    readInto(async () => Promise.reject(new Error('rate limit exceeded')), node, (s) => states.push(s));
    readInto(async () => Promise.reject('offline'), node, (s) => states.push(s));
    await settled();

    expect(states.slice(2)).toEqual([
      { status: 'error', message: 'rate limit exceeded' },
      { status: 'error', message: 'offline' },
    ]);
  });

  it('reports nothing once it was stopped: the reply is for a node that is no longer shown', async () => {
    const states: SourceState[] = [];

    const stop = readInto(async () => source, node, (s) => states.push(s));
    const stopFailed = readInto(async () => Promise.reject(new Error('late')), node, (s) => states.push(s));
    stop();
    stopFailed();
    await settled();

    expect(states).toEqual([{ status: 'reading' }, { status: 'reading' }]);
  });
});

describe('sourceReader and the forge', () => {
  it('asks for the forge at each read, so that a token given or forgotten since the last one counts', async () => {
    const files = { [HEAD]: { 'A.java': 'x\n', 'B.java': 'y\n' } };
    const first = fakeForge(files);
    const second = fakeForge(files);
    let now = first.forge;
    const cache = memoryCache();
    const read = sourceReader({ forge: () => now, repo: REPO, cache: async () => cache, head: HEAD, base: undefined });

    await read({ file: 'A.java', span: span(1, 1) });
    now = second.forge;
    await read({ file: 'B.java', span: span(1, 1) });

    expect(first.calls).toEqual(['tree h', 'text h A.java']);
    expect(second.calls).toEqual(['text h B.java']);
  });
});

describe('sourceReader with an overlay', () => {
  it('marks the lines of a modified node with the changes of the commit', async () => {
    const { read } = reader(
      { [BASE]: { 'A.java': 'one\ntwo\nthree\n' }, [HEAD]: { 'A.java': 'one\nTWO\nthree\n' } },
      BASE,
    );

    const source = await read({ file: 'A.java', span: span(1, 3), change: 'modified' });

    expect(shown(source.lines)).toEqual(['1 one =', '2 two -', '2 TWO +', '3 three =']);
  });

  it('gives the lines of the base for a removed node, also when the head has no such file', async () => {
    const { read } = reader({ [BASE]: { 'A.java': 'one\ntwo\nthree\n', 'B.java': 'x\ny\n' }, [HEAD]: { 'A.java': 'one\nthree\n' } }, BASE);

    const kept = await read({ file: 'A.java', span: span(1, 3), change: 'removed' });
    const gone = await read({ file: 'B.java', span: span(1, 2), change: 'removed' });

    expect(shown(kept.lines)).toEqual(['1 one =', '2 two -', '3 three =']);
    expect(shown(gone.lines)).toEqual(['1 x -', '2 y -']);
  });

  it('marks all lines as added for a file that the base lacks and for a root commit', async () => {
    const files: Record<string, Record<string, string>> = { [BASE]: {}, [HEAD]: { 'A.java': 'one\ntwo\n' } };
    const added = await reader(files, BASE).read({ file: 'A.java', span: span(1, 2), change: 'added' });
    const root = await reader(files, null).read({ file: 'A.java', span: span(1, 2), change: 'added' });

    expect(shown(added.lines)).toEqual(['1 one +', '2 two +']);
    expect(shown(root.lines)).toEqual(['1 one +', '2 two +']);
  });

  it('marks nothing in a file that the commit left alone, and reads its text once', async () => {
    const same = { 'A.java': 'one\ntwo\n' };
    const { read, calls } = reader({ [BASE]: same, [HEAD]: same }, BASE);

    const source = await read({ file: 'A.java', span: span(1, 2), change: 'modified' });

    expect(shown(source.lines)).toEqual(['1 one =', '2 two =']);
    expect(calls.filter((c) => c.startsWith('text'))).toEqual(['text h A.java']);
  });
});

describe('sourceReader failures', () => {
  it('rejects for a path that the commit lacks, with the short sha', async () => {
    const plain = reader({ [HEAD]: { 'B.java': 'x\n' } }, undefined);
    await expect(plain.read({ file: 'A.java', span: span(1, 1) })).rejects.toThrow('no file A.java at hhhhhhh');

    const overlay = reader({ [BASE]: { 'B.java': 'x\n' }, [HEAD]: { 'A.java': 'x\n' } }, BASE);
    await expect(overlay.read({ file: 'A.java', span: span(1, 1), change: 'removed' })).rejects.toThrow('no file A.java at bbbbbbb');
    await expect(overlay.read({ file: 'B.java', span: span(1, 1), change: 'modified' })).rejects.toThrow('no file B.java at hhhhhhh');
  });

  it('rejects for a node without a file or without a span', async () => {
    const { read } = reader({ [HEAD]: { 'A.java': 'x\n' } }, undefined);

    await expect(read({ file: 'A.java' })).rejects.toThrow('no source lines');
    await expect(read({ span: span(1, 1) })).rejects.toThrow('no source lines');
  });

  it('rejects with the message of the forge when it fails to give a text', async () => {
    const { forge } = fakeForge({ [HEAD]: { 'A.java': 'x\n' } }, 'rate limit exceeded');
    const read = sourceReader({ forge: () => forge, repo: REPO, cache: async () => memoryCache(), head: HEAD, base: undefined });

    await expect(read({ file: 'A.java', span: span(1, 1) })).rejects.toThrow('rate limit exceeded');
  });

  it('asks the forge again after it failed to give the file list or a text', async () => {
    const { forge, calls } = fakeForge({ [HEAD]: { 'A.java': 'x\n' } });
    const works = { tree: forge.tree, text: forge.text };
    const failing = new Set(['tree', 'text']);
    forge.tree = async (repo, sha) => {
      if (failing.delete('tree')) throw new Error('no file list');
      return works.tree(repo, sha);
    };
    forge.text = async (repo, sha, path) => {
      if (failing.delete('text')) throw new Error('no text');
      return works.text(repo, sha, path);
    };
    const cache = memoryCache();
    const read = sourceReader({ forge: () => forge, repo: REPO, cache: async () => cache, head: HEAD, base: undefined });
    const node = { file: 'A.java', span: span(1, 1) };

    await expect(read(node)).rejects.toThrow('no file list');
    await expect(read(node)).rejects.toThrow('no text');
    expect(shown((await read(node)).lines)).toEqual(['1 x =']);
    expect(calls).toEqual(['tree h', 'text h A.java']);
  });
});

describe('overlayLines at the base', () => {
  it('gives the span of the base, marks the lines that the head lacks, and leaves out the added lines', () => {
    const lines = overlayLines(text('abcd'), text('acXd'), span(2, 4), 'base');

    expect(shown(lines)).toEqual(['2 b -', '3 c =', '4 d =']);
  });

  it('marks every line as removed when the head has no such file', () => {
    const lines = overlayLines(text('abc'), undefined, span(1, 2), 'base');

    expect(shown(lines)).toEqual(['1 a -', '2 b -']);
  });
});
