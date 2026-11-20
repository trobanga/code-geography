// The source view (ADR 0031): the lines of a selected box, or the declarations behind the line
// between two selected boxes. The pure parts are here; the texts come from a forge by blob sha.

import type { KindSwitch } from './edgeOptions';
import { parentOf, type Tree } from './expansion';
import { cachedTree, texts, type Entry, type Forge, type Repo, type TextCache } from './forge';
import { lineDiff } from './lineDiff';
import { EDGE_KINDS, type EdgeChange, type EdgeKind, type GraphEdge, type GraphNode } from './types';

/** A span of lines in a file: 1-based, inclusive. */
export type Span = NonNullable<GraphNode['span']>;

/** The span of a whole file: every line, and under an overlay each removed line at its place. */
export const WHOLE_FILE: Span = { start_line: 0, end_line: Number.MAX_SAFE_INTEGER };

/** What a commit did to a line. */
export type LineMark = EdgeChange;

/**
 * One line of a shown span. `no` is its number in its file: the file of the head, and for a
 * removed line the file of the base.
 */
export interface SourceLine {
  no: number;
  text: string;
  mark: LineMark | null;
  /** Its number in the file at the head; absent on a line that the commit removed. */
  head?: number;
  /** Its number in the file at the base of an overlay; absent on an added line, without an overlay, and when the base has no file of this path. */
  base?: number;
  /** The references that stand on this line of the source: sites of the edges behind the line between the two selected boxes (ADR 0032). */
  refs?: LineRef[];
  /** In a block of single lines: the line does not directly follow the line shown before it. */
  gap?: true;
}

/** A reference on a line: the target and the kind of its edge, and what the commit did to the edge. */
export interface LineRef {
  to: string;
  kind: EdgeKind;
  change: EdgeChange | null;
}

/** The lines of a text; a line end at the end of the text starts no further line. `\r\n` ends a line too. */
export function splitLines(text: string): string[] {
  const lines = text.split(/\r?\n/);
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/**
 * The lines of a declaration under a commit overlay, from the line diff of its file.
 * `side: 'head'`: the lines `span` of `head`; a line that the commit added is marked, and the
 * lines that it removed stand where they were, marked, with their number in `base`.
 * `side: 'base'`: a declaration that the commit removed: the lines `span` of `base`; a line
 * that the head no longer has is marked removed.
 * `undefined`: the commit has no such file on that side.
 */
export function overlayLines(base: string | undefined, head: string | undefined, span: Span, side: 'base' | 'head'): SourceLine[] {
  const baseLines = splitLines(base ?? '');
  const headLines = splitLines(head ?? '');
  const diff = lineDiff(baseLines, headLines);
  const inSpan = (n: number | null) => n !== null && n >= span.start_line && n <= span.end_line;
  if (side === 'base') {
    return diff
      .filter((l) => l.base !== null && inSpan(l.base))
      .map((l): SourceLine => {
        const line: SourceLine = { no: l.base!, text: baseLines[l.base! - 1], mark: l.head === null ? 'removed' : null, base: l.base! };
        if (l.head !== null) line.head = l.head;
        return line;
      });
  }
  const out: SourceLine[] = [];
  // For a removed line: the number that the next line of the head has, where it stood.
  let position = headLines.length + 1;
  const positions: number[] = new Array(diff.length);
  for (let i = diff.length - 1; i >= 0; i--) {
    const h = diff[i].head;
    if (h !== null) position = h;
    positions[i] = position;
  }
  const startIsAdded = diff.some((l) => l.head === span.start_line && l.base === null);
  diff.forEach((l, i) => {
    if (l.head !== null) {
      if (inSpan(l.head)) {
        const line: SourceLine = { no: l.head, text: headLines[l.head - 1], mark: l.base === null ? 'added' : null, head: l.head };
        if (l.base !== null) line.base = l.base;
        out.push(line);
      }
    } else if (l.base !== null) {
      const p = positions[i];
      const inside = p > span.start_line && p <= span.end_line;
      if (inside || (p === span.start_line && startIsAdded)) out.push({ no: l.base, text: baseLines[l.base - 1], mark: 'removed', base: l.base });
    }
  });
  return out;
}

/** The node has lines of its own: a file and a span in it. A module or a package has none. */
export function hasLines<N extends Pick<GraphNode, 'file' | 'span'>>(node: N): node is N & { file: string; span: Span } {
  return node.file !== undefined && node.span !== undefined;
}

/**
 * The declarations that the source view lists for a container without lines of its own: below
 * `id`, each topmost node with lines, in tree order (depth first). Without `tests`, no test code.
 */
export function declarations(tree: Tree, id: string, tests: boolean): string[] {
  const out: string[] = [];
  const visit = (parent: string) => {
    for (const child of tree.children.get(parent) ?? []) {
      const node = tree.byId.get(child);
      if (!node || (node.test && !tests)) continue;
      if (hasLines(node)) out.push(child);
      else visit(child);
    }
  };
  visit(id);
  return out;
}

/** Rows that a list of the source view shows at first, and adds on request. */
export const PAGE = 100;

/**
 * What stands before the name of a declaration in a list: the label of its parent and a dot.
 * Empty for a node of a module: a module is no part of a qualified name.
 */
export function qualifier(tree: Tree, id: string): string {
  const parent = parentOf(tree, id);
  return parent !== null && tree.byId.get(parent)?.kind !== 'module' ? `${tree.label.get(parent) ?? parent}.` : '';
}

/** One target of a declaration on a line: the kinds of its edges in the order of EDGE_KINDS, and the mark if all of its edges carry the same one. */
export interface Target {
  id: string;
  kinds: EdgeKind[];
  change: EdgeChange | null;
}

/** A declaration that makes a line, with what it uses on the other end. */
export interface LineGroup {
  from: string;
  targets: Target[];
  /** Its edges (of the kinds switched on), in the order given. */
  edges: GraphEdge[];
}

/**
 * The edges behind a line by their `from` declaration, in the order of first appearance, each
 * with its targets in the order of first appearance. Edges of a kind that is switched off are
 * left out, like in the picture.
 */
export function lineGroups(edges: readonly GraphEdge[], kinds: KindSwitch): LineGroup[] {
  const byFrom = new Map<string, { edges: GraphEdge[]; targets: Map<string, GraphEdge[]> }>();
  for (const e of edges) {
    if (!kinds[e.kind]) continue;
    let group = byFrom.get(e.from);
    if (!group) byFrom.set(e.from, (group = { edges: [], targets: new Map() }));
    group.edges.push(e);
    const list = group.targets.get(e.to);
    if (list) list.push(e);
    else group.targets.set(e.to, [e]);
  }
  return [...byFrom].map(([from, group]) => ({
    from,
    edges: group.edges,
    targets: [...group.targets].map(([id, list]) => ({
      id,
      kinds: EDGE_KINDS.filter((k) => list.some((e) => e.kind === k)),
      change: list.every((e) => e.change === list[0].change) ? (list[0].change ?? null) : null,
    })),
  }));
}

/** A number with its noun: `1 declaration`, `3 declarations`. */
export function counted(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}

/** What the source view says about one direction of a line: its declarations and their targets. */
export function lineSummary(groups: readonly LineGroup[]): string {
  if (groups.length === 0) return 'nothing';
  const targets = groups.reduce((sum, g) => sum + g.targets.length, 0);
  return `${counted(groups.length, 'declaration')}, ${counted(targets, 'target')}`;
}

/** What the source view shows for a selection. */
export type SourceView =
  /** One box with lines of its own. */
  | { kind: 'code'; node: string }
  /** One container: its declarations, as rows without targets. */
  | { kind: 'list'; of: string; rows: LineGroup[] }
  /** Two boxes: the edges behind the line a → b and behind the line b → a, for `lineGroups`. */
  | { kind: 'pair'; a: string; b: string; there: GraphEdge[]; back: GraphEdge[] };

/**
 * `between(from, to)`: the edges behind the line of two boxes (the WASM `Explorer.edgesBetween`).
 * Null: nothing to show (no selection, three or more boxes, an id that is no node of the tree).
 */
export function sourceView(
  tree: Tree,
  selection: readonly string[],
  tests: boolean,
  between: (from: string, to: string) => GraphEdge[],
): SourceView | null {
  if (selection.length === 0 || selection.length > 2 || !selection.every((id) => tree.byId.has(id))) return null;
  if (selection.length === 2) {
    const [a, b] = selection;
    return { kind: 'pair', a, b, there: between(a, b), back: between(b, a) };
  }
  const id = selection[0];
  if (hasLines(tree.byId.get(id)!)) return { kind: 'code', node: id };
  return { kind: 'list', of: id, rows: declarations(tree, id, tests).map((from) => ({ from, targets: [], edges: [] })) };
}

/**
 * The lines `span` of a text, numbered as in the file, without marks. Each line has its number
 * at the head; with `both` (the same blob on both sides of an overlay) also at the base.
 */
export function spanLines(text: string, span: Span, sides: 'head' | 'both' = 'head'): SourceLine[] {
  return splitLines(text)
    .slice(Math.max(span.start_line, 1) - 1, span.end_line)
    .map((line, i) => {
      const no = Math.max(span.start_line, 1) + i;
      return { no, text: line, mark: null, head: no, ...(sides === 'both' ? { base: no } : {}) };
    });
}

/** Where the texts of the graph shown come from: a repository of a forge at one commit, or at the two commits of an overlay. */
export interface SourceRepo {
  /** Asked for at each read, so that it carries the token of its host as stored then (ADR 0022). */
  forge: () => Forge;
  repo: Repo;
  cache: () => Promise<TextCache>;
  /** The sha of the commit shown. */
  head: string;
  /** The base of the overlay shown; null: the head is a root commit; undefined: no overlay. */
  base: string | null | undefined;
}

export interface NodeSource {
  file: string;
  lines: SourceLine[];
}

/** Reads the lines of one node; rejects with a message for the user. */
export type ReadSource = (node: Pick<GraphNode, 'file' | 'span' | 'change'>) => Promise<NodeSource>;

/**
 * The reader of one graph shown. A text comes from the cache by blob sha, else from the forge
 * (and then goes into the cache); the file list of a commit is read once per reader.
 * Without an overlay: the lines of the span at the head. With one: the marks of the commit
 * (`overlayLines`); a removed node takes its lines from the base. A file that the base lacks
 * under its path has all its lines marked as added only for a node that the commit added; for
 * any other node (the file moved) its lines have no marks.
 */
export function sourceReader(d: SourceRepo): ReadSource {
  const trees = new Map<string, Promise<Map<string, Entry>>>();
  const entriesAt = (sha: string): Promise<Map<string, Entry>> => {
    let entries = trees.get(sha);
    if (!entries) {
      entries = d
        .cache()
        .then((cache) => cachedTree(d.forge(), d.repo, sha, cache))
        .then((list) => new Map(list.map((e) => [e.path, e])));
      // A failure is not kept: the next read asks again.
      entries.catch(() => trees.delete(sha));
      trees.set(sha, entries);
    }
    return entries;
  };
  const fileAt = (sha: string) => async (path: string) => {
    const entry = (await entriesAt(sha)).get(path);
    return entry && { key: entry.blobSha, text: async () => (await texts(d.forge(), d.repo, sha, [entry], await d.cache()))[0].text };
  };
  const base = d.base === undefined ? undefined : d.base === null ? null : fileAt(d.base);
  return readerOf({ head: fileAt(d.head), base, at: (side) => ` at ${(side === 'head' ? d.head : d.base!).slice(0, 7)}` });
}

/**
 * The texts of a local directory, by path below it, kept in memory from the index on: a file
 * that changed since then would have spans that no longer fit (ADR 0031). `head`: the texts that
 * were indexed. `base`: the texts of the inputs at `HEAD`, for the overlay of uncommitted changes (ADR 0039).
 */
export interface SourceTexts {
  head: ReadonlyMap<string, string>;
  base?: ReadonlyMap<string, string>;
}

/** The reader of a local directory: the lines of the span, from the texts held; with a base, the marks of the uncommitted changes. */
export function textReader(t: SourceTexts): ReadSource {
  const fileIn = (texts: ReadonlyMap<string, string>) => async (path: string) => {
    const text = texts.get(path);
    return text === undefined ? undefined : { key: text, text: async () => text };
  };
  return readerOf({ head: fileIn(t.head), base: t.base && fileIn(t.base), at: (side) => (side === 'base' ? ' at HEAD' : '') });
}

/** One file of one side: what names its content (equal keys, equal texts) and how to read it. */
interface SideFile {
  key: string;
  text: () => Promise<string>;
}

type FileAt = (path: string) => Promise<SideFile | undefined>;

/** The two sides of a reader. `base` null: the head is a root commit; undefined: no overlay. `at`: where a side is, for a message. */
interface Sides {
  head: FileAt;
  base: FileAt | null | undefined;
  at: (side: 'base' | 'head') => string;
}

/** Without an overlay: the lines of the span at the head. With one: the marks of the commit; see `sourceReader`. */
function readerOf(s: Sides): ReadSource {
  return async (node) => {
    if (!hasLines(node)) throw new Error('no source lines');
    const { file, span } = node;
    // The base has no such file for an added file, and there is no base for a root commit or without an overlay.
    const hasBase = s.base !== null && s.base !== undefined;
    const [base, head] = await Promise.all([hasBase ? s.base!(file) : undefined, s.head(file)]);
    // A node that the commit removed, or a file that only the base has (a reference in a file that the commit deleted).
    const removed = hasBase && (node.change === 'removed' || (!head && base !== undefined));
    if (!(removed ? base : head)) throw new Error(`no file ${file}${s.at(removed ? 'base' : 'head')}`);
    // The base has no file of this path, but the commit did not add the node: the file moved (the ids of
    // its nodes stay), or it is a new file of a node that was there. The graph does not hold the path at
    // the base, so no line is marked, and none has a number at the base.
    const baseUnknown = hasBase && !removed && base === undefined && node.change !== 'added';
    // The same text on both sides, or one side only: one text, no marks.
    const shown = removed ? base! : head!;
    if (s.base === undefined || baseUnknown || base?.key === head?.key) {
      return { file, lines: spanLines(await shown.text(), span, s.base === undefined || baseUnknown ? 'head' : 'both') };
    }
    const [baseText, headText] = await Promise.all([base?.text(), head?.text()]);
    return { file, lines: overlayLines(baseText, headText, span, removed ? 'base' : 'head') };
  };
}

/** Some edge has a site entry with a line: the source says where it makes the edge. */
export function hasSites(edges: readonly GraphEdge[]): boolean {
  return edges.some((e) => e.sites?.some((site) => site.lines.length > 0));
}

/**
 * What a row of a list needs to open: the node and the reader. A row opens when the graph has
 * texts (`read`) and its declaration has lines of its own or its edges have sites (a package
 * with an `imports` edge, ADR 0032). Null: the row does not open.
 */
export function openable<N extends Pick<GraphNode, 'file' | 'span'>>(
  read: ReadSource | null,
  node: N | undefined,
  edges: readonly GraphEdge[],
): { node: N; read: ReadSource } | null {
  return read !== null && node !== undefined && (hasLines(node) || hasSites(edges)) ? { node, read } : null;
}

/** The references of a line as one text for a tooltip: `calls run, type_ref Bar`. */
export function refsTitle(refs: readonly LineRef[], nameOf: (id: string) => string): string {
  return refs.map((r) => `${r.kind} ${nameOf(r.to)}`).join(', ');
}

/** One part of what an open declaration shows. `own`: the lines of the declaration itself; else lines of `file` outside them that hold a reference. */
export interface SourceBlock {
  file: string;
  own: boolean;
  lines: SourceLine[];
}

/**
 * What an open declaration shows (ADR 0032). First its own lines, if it has any (`read(node)`), with
 * the references of `edges` marked. Then, per file in the order of first appearance in the sites
 * (the file of the node first), a block of the lines with a reference that the own lines do not
 * hold: an import above a type, an `impl` block, the `ns` form of a namespace, a file other than
 * that of the declaration. Such a block comes from one read of the whole file (`WHOLE_FILE`), keeps
 * only the lines with a reference that was not placed in the own lines, and sets `gap` on each
 * line but the first that does not directly follow the line kept before it in the file.
 * Without sites: the own lines only. Rejects like `read` for a node without lines whose edges have no site.
 */
export async function readDeclaration(read: ReadSource, node: Pick<GraphNode, 'file' | 'span' | 'change'>, edges: readonly GraphEdge[]): Promise<SourceBlock[]> {
  // A node without lines and without sites has nothing to show: the reader rejects it.
  const own = hasLines(node) || !hasSites(edges) ? await read(node) : undefined;
  const blocks: SourceBlock[] = own ? [{ file: own.file, own: true, lines: withRefs(own.lines, own.file, edges, node.file) }] : [];
  // The numbers of the own lines on each side: a site with such a number is placed there.
  const placed = { head: new Set(own?.lines.map((l) => l.head)), base: new Set(own?.lines.map((l) => l.base)) };
  // The sites that the own lines do not hold, by file; for each: the edge with its entries cut down to those lines.
  const outside = new Map<string, GraphEdge[]>();
  for (const e of edges) {
    const side = placed[siteSide(e)];
    for (const site of e.sites ?? []) {
      const file = site.file ?? node.file;
      if (file === undefined) continue;
      const lines = file === own?.file ? site.lines.filter((n) => !side.has(n)) : site.lines;
      if (lines.length === 0) continue;
      const list = outside.get(file);
      if (list) list.push({ ...e, sites: [{ file, lines }] });
      else outside.set(file, [{ ...e, sites: [{ file, lines }] }]);
    }
  }
  // The file of the node first, the others in the order of first appearance.
  const order = [...outside].sort(([a], [b]) => Number(b === node.file) - Number(a === node.file));
  // With the change of the declaration: a file that the base lacks is new only if the declaration is.
  const wholes = await Promise.all(order.map(([file]) => read({ file, span: WHOLE_FILE, change: node.change })));
  order.forEach(([file, cut], at) => {
    const kept = withRefs(wholes[at].lines, file, cut, node.file).flatMap((line, i) => (line.refs ? [{ line, i }] : []));
    const lines = kept.map(({ line, i }, k) => (k > 0 && i !== kept[k - 1].i + 1 ? { ...line, gap: true as const } : line));
    if (lines.length > 0) blocks.push({ file, own: false, lines });
  });
  return blocks;
}

/** The side of an overlay that holds the sites of an edge: the base for an edge that the commit removed, else the head (ADR 0032). */
function siteSide(edge: GraphEdge): 'base' | 'head' {
  return edge.change === 'removed' ? 'base' : 'head';
}

/** What a read of the lines of one node gave so far. */
export type SourceState = { status: 'reading' } | { status: 'lines'; blocks: SourceBlock[] } | { status: 'error'; message: string };

/**
 * Reads what a declaration shows (`readDeclaration`) and reports each state: `reading` at once,
 * then the blocks or the message of the failure. The result stops the reports: a reply that comes later is for a node
 * that is no longer shown.
 */
export function readInto(
  read: ReadSource,
  node: Parameters<ReadSource>[0],
  edges: readonly GraphEdge[],
  onState: (s: SourceState) => void,
): () => void {
  let stopped = false;
  const report = (s: SourceState) => {
    if (!stopped) onState(s);
  };
  report({ status: 'reading' });
  readDeclaration(read, node, edges).then(
    (blocks) => report({ status: 'lines', blocks }),
    (e) => report({ status: 'error', message: e instanceof Error ? e.message : String(e) }),
  );
  return () => {
    stopped = true;
  };
}

/**
 * The lines of `file` with the references of `edges` that stand on them. A site entry without a
 * file is in `fromFile`. An edge that the commit removed has its sites in the base
 * (`SourceLine.base`), every other edge in the head (`SourceLine.head`). The refs of a line are in
 * the order of the edges. A line without a reference comes back as it is.
 */
export function withRefs(lines: readonly SourceLine[], file: string, edges: readonly GraphEdge[], fromFile: string | undefined): SourceLine[] {
  // The refs by line number on each side, each with the position of its edge.
  type Placed = { at: number; ref: LineRef };
  const sides = { head: new Map<number, Placed[]>(), base: new Map<number, Placed[]>() };
  edges.forEach((e, at) => {
    const side = sides[siteSide(e)];
    for (const site of e.sites ?? []) {
      if ((site.file ?? fromFile) !== file) continue;
      for (const n of site.lines) {
        const placed: Placed = { at, ref: { to: e.to, kind: e.kind, change: e.change ?? null } };
        const list = side.get(n);
        if (list) list.push(placed);
        else side.set(n, [placed]);
      }
    }
  });
  return lines.map((l) => {
    const placed = [...(l.head === undefined ? [] : (sides.head.get(l.head) ?? [])), ...(l.base === undefined ? [] : (sides.base.get(l.base) ?? []))];
    return placed.length === 0 ? l : { ...l, refs: placed.sort((a, b) => a.at - b.at).map((p) => p.ref) };
  });
}
