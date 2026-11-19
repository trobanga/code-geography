// The text fields of the commit list and of the branch menu: which rows a query keeps.
import type { Branch } from './forge';
import type { CommitRow } from './forgeDiff';

/** The words of a query, in lower case. */
export const words = (query: string) => query.toLowerCase().split(/\s+/).filter(Boolean);

/**
 * The rows for which each word of the query is in the title, the author or the day, or is the
 * start of the sha; an empty query keeps all.
 */
export function filterCommits(rows: CommitRow[], query: string): CommitRow[] {
  const ws = words(query);
  return rows.filter((r) => {
    const text = `${r.title}\n${r.author}\n${r.day}`.toLowerCase();
    return ws.every((w) => text.includes(w) || r.sha.startsWith(w));
  });
}

/** The query as a full or short sha: 7 to 40 hex digits, in lower case; null for anything else. */
export function shaOf(query: string): string | null {
  const q = query.trim().toLowerCase();
  return /^[0-9a-f]{7,40}$/.test(q) ? q : null;
}

/** Alphabetical, capitals not apart; equal names in lower case by code point, so the order is total. */
const byName = (a: Branch, b: Branch) => {
  const x = a.name.toLowerCase();
  const y = b.name.toLowerCase();
  return x < y ? -1 : x > y ? 1 : a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
};

/** The characters of `q` occur in `text` in this order, with gaps or not. */
function inOrder(text: string, q: string): boolean {
  let at = 0;
  for (const ch of q) {
    at = text.indexOf(ch, at) + 1;
    if (at === 0) return false;
  }
  return true;
}

/**
 * The branches for the text field of the branch menu, in alphabetical order: those whose name
 * holds the characters of the query in this order (a fuzzy match, capitals not apart, spaces of
 * the query not counted). The names that hold the query as one block go first. An empty query
 * keeps all.
 */
export function filterBranches(branches: Branch[], query: string): Branch[] {
  const q = words(query).join('');
  const sorted = [...branches].sort(byName);
  if (q === '') return sorted;
  const block: Branch[] = [];
  const gaps: Branch[] = [];
  for (const b of sorted) {
    const name = b.name.toLowerCase();
    if (name.includes(q)) block.push(b);
    else if (inOrder(name, q)) gaps.push(b);
  }
  return [...block, ...gaps];
}
