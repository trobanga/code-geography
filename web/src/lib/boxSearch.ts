// The box search (ADR 0038): which nodes a query finds, best first.
import { inTestCode } from './commit';
import { pathToOpen, type Tree } from './expansion';
import { words } from './menuSearch';
import type { NodeKind } from './types';

export interface BoxEntry {
  id: string;
  name: string;
  lower: string;
  kind: NodeKind;
  /** Labels of the ancestors, top down, joined with ' › '; '' for a root. */
  path: string;
  test: boolean;
}

/** One entry per node of the tree, computed once per graph. */
export function boxIndex(tree: Tree): BoxEntry[] {
  return [...tree.byId.values()].map((n) => ({
    id: n.id,
    name: n.name,
    lower: n.name.toLowerCase(),
    kind: n.kind,
    path: pathToOpen(tree, n.id)
      .map((p) => tree.label.get(p) ?? p)
      .join(' › '),
    test: inTestCode(tree, n.id),
  }));
}

export interface BoxMatch {
  id: string;
  name: string;
  kind: NodeKind;
  path: string;
  /** In test code while test code is hidden. */
  hidden: boolean;
}

/** By code point, so that the order is total and does not depend on the locale. */
const byText = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/**
 * Matches for the query, best first, at most `limit`; `total` counts all matches. Each word of
 * the query must be in the name, capitals not apart. Order: while test code is hidden, test code
 * last; the name equal to the query, then names that start with its first word, then the rest;
 * shorter names first; then path and id.
 */
export function findBoxes(index: BoxEntry[], query: string, tests: boolean, limit = 50): { matches: BoxMatch[]; total: number } {
  const ws = words(query);
  if (ws.length === 0) return { matches: [], total: 0 };
  const whole = ws.join(' ');
  const rank = (e: BoxEntry) => (e.lower === whole ? 0 : e.lower.startsWith(ws[0]) ? 1 : 2);
  const hidden = (e: BoxEntry) => !tests && e.test;
  const cmp = (a: BoxEntry, b: BoxEntry) =>
    Number(hidden(a)) - Number(hidden(b)) ||
    rank(a) - rank(b) ||
    a.name.length - b.name.length ||
    byText(a.path, b.path) ||
    byText(a.id, b.id);
  const found = index.filter((e) => ws.every((w) => e.lower.includes(w))).sort(cmp);
  const matches = found.slice(0, limit).map((e) => ({ id: e.id, name: e.name, kind: e.kind, path: e.path, hidden: hidden(e) }));
  return { matches, total: found.length };
}
