// The source view (ADR 0031): the lines of a selected box, or the declarations behind the line
// between two selected boxes. The pure parts are here; the texts come from a forge by blob sha.

import type { KindSwitch } from './edgeOptions';
import { parentOf, type Tree } from './expansion';
import { cachedTree, texts, type Entry, type Forge, type Repo, type TextCache } from './forge';
import { lineDiff } from './lineDiff';
import { EDGE_KINDS, type EdgeChange, type EdgeKind, type GraphEdge, type GraphNode } from './types';

/** A span of lines in a file: 1-based, inclusive. */
export type Span = NonNullable<GraphNode['span']>;

/** What a commit did to a line. */
export type LineMark = EdgeChange;

/**
 * One line of a shown span. `no` is its number in its file: the file of the head, and for a
 * removed line the file of the base. Further marks of a line (the place of a reference) can
 * be added as fields.
 */
export interface SourceLine {
  no: number;
  text: string;
  mark: LineMark | null;
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
      .map((l) => ({ no: l.base!, text: baseLines[l.base! - 1], mark: l.head === null ? 'removed' : null }));
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
      if (inSpan(l.head)) out.push({ no: l.head, text: headLines[l.head - 1], mark: l.base === null ? 'added' : null });
    } else if (l.base !== null) {
      const p = positions[i];
      const inside = p > span.start_line && p <= span.end_line;
      if (inside || (p === span.start_line && startIsAdded)) out.push({ no: l.base, text: baseLines[l.base - 1], mark: 'removed' });
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
}

/**
 * The edges behind a line by their `from` declaration, in the order of first appearance, each
 * with its targets in the order of first appearance. Edges of a kind that is switched off are
 * left out, like in the picture.
 */
export function lineGroups(edges: readonly GraphEdge[], kinds: KindSwitch): LineGroup[] {
  const byFrom = new Map<string, Map<string, GraphEdge[]>>();
  for (const e of edges) {
    if (!kinds[e.kind]) continue;
    let targets = byFrom.get(e.from);
    if (!targets) byFrom.set(e.from, (targets = new Map()));
    const list = targets.get(e.to);
    if (list) list.push(e);
    else targets.set(e.to, [e]);
  }
  return [...byFrom].map(([from, targets]) => ({
    from,
    targets: [...targets].map(([id, list]) => ({
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
  return { kind: 'list', of: id, rows: declarations(tree, id, tests).map((from) => ({ from, targets: [] })) };
}

/** The lines `span` of a text, numbered as in the file, without marks. */
export function spanLines(text: string, span: Span): SourceLine[] {
  return splitLines(text)
    .slice(Math.max(span.start_line, 1) - 1, span.end_line)
    .map((line, i) => ({ no: Math.max(span.start_line, 1) + i, text: line, mark: null }));
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
 * (`overlayLines`); a removed node takes its lines from the base.
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
  const entryAt = async (sha: string, path: string) => (await entriesAt(sha)).get(path);
  const textOf = async (sha: string, entry: Entry) => (await texts(d.forge(), d.repo, sha, [entry], await d.cache()))[0].text;
  return async (node) => {
    if (!hasLines(node)) throw new Error('no source lines');
    const { file, span } = node;
    // The base has no such file for an added file, and there is no base for a root commit or without an overlay.
    const baseSha = d.base ?? null;
    const [base, head] = await Promise.all([baseSha === null ? undefined : entryAt(baseSha, file), entryAt(d.head, file)]);
    const removed = baseSha !== null && node.change === 'removed';
    if (!(removed ? base : head)) throw new Error(`no file ${file} at ${(removed ? baseSha : d.head).slice(0, 7)}`);
    // The same blob on both sides, or one side only without an overlay: one text, no marks.
    const shown = removed ? { sha: baseSha, entry: base! } : { sha: d.head, entry: head! };
    if (d.base === undefined || base?.blobSha === head?.blobSha) return { file, lines: spanLines(await textOf(shown.sha, shown.entry), span) };
    const [baseText, headText] = await Promise.all([base && textOf(baseSha!, base), head && textOf(d.head, head)]);
    return { file, lines: overlayLines(baseText, headText, span, removed ? 'base' : 'head') };
  };
}

/** What a read of the lines of one node gave so far. */
export type SourceState = { status: 'reading' } | { status: 'lines'; source: NodeSource } | { status: 'error'; message: string };

/**
 * Reads the lines of a node and reports each state: `reading` at once, then the lines or the
 * message of the failure. The result stops the reports: a reply that comes later is for a node
 * that is no longer shown.
 */
export function readInto(read: ReadSource, node: Parameters<ReadSource>[0], onState: (s: SourceState) => void): () => void {
  let stopped = false;
  const report = (s: SourceState) => {
    if (!stopped) onState(s);
  };
  report({ status: 'reading' });
  read(node).then(
    (source) => report({ status: 'lines', source }),
    (e) => report({ status: 'error', message: e instanceof Error ? e.message : String(e) }),
  );
  return () => {
    stopped = true;
  };
}
