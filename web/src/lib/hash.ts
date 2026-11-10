import { DEFAULT_EDGE_OPTIONS, type EdgeOptions } from './edgeOptions';
import type { Focus } from './focus';
import { EDGE_KINDS, LEVELS, type EdgeKind, type Level } from './types';

export const SOURCES = ['graph', 'commit'] as const;
/** Which file under public/ is loaded: graph.json or commit.json. */
export type Source = (typeof SOURCES)[number];

export interface HashState {
  src?: Source;
  level: Level;
  tests: boolean;
  open: string[];
  edges: EdgeOptions;
  /** Absent: no focus. */
  focus?: Focus;
}

const FLAGS = [
  ['sel', 'onSelection'],
  ['lift', 'lift'],
  ['merge', 'mergeImports'],
  ['changes', 'changesOnly'],
] as const;

export function encodeHash(s: HashState): string {
  const parts = [`level=${s.level}`, `tests=${s.tests ? 1 : 0}`];
  if (s.src) parts.unshift(`src=${s.src}`);
  if (s.open.length > 0) parts.push(`open=${s.open.map(encodeURIComponent).join(',')}`);
  const on = EDGE_KINDS.filter((k) => s.edges.kinds[k]);
  if (on.length < EDGE_KINDS.length) parts.push(`kinds=${on.length > 0 ? on.join(',') : 'none'}`);
  // A flag is written only when it differs from its default; removed keys are ignored on load.
  for (const [key, field] of FLAGS)
    if (s.edges[field] !== DEFAULT_EDGE_OPTIONS[field]) parts.push(`${key}=${s.edges[field] ? 1 : 0}`);
  const ids = (xs: string[]) => xs.map(encodeURIComponent).join(',');
  const f = s.focus;
  if (f?.nodes.length) parts.push(`focus=${ids(f.nodes)}`);
  if (f?.changes) parts.push('fchanges=1');
  if (f?.neighbors) parts.push('fnear=1');
  if (f?.unfolded.length) parts.push(`unfold=${ids(f.unfolded)}`);
  return '#' + parts.join('&');
}

function decodeKinds(value: string): Record<EdgeKind, boolean> | null {
  const names = value === 'none' ? [] : value.split(',').filter((k) => (EDGE_KINDS as readonly string[]).includes(k));
  if (value !== 'none' && names.length === 0) return null;
  return Object.fromEntries(EDGE_KINDS.map((k) => [k, names.includes(k)])) as Record<EdgeKind, boolean>;
}

export function decodeHash(hash: string): HashState {
  const edges: EdgeOptions = { ...DEFAULT_EDGE_OPTIONS, kinds: { ...DEFAULT_EDGE_OPTIONS.kinds } };
  const state: HashState = { level: 'modules', tests: false, open: [], edges };
  const focus = (): Focus => (state.focus ??= { nodes: [], changes: false, neighbors: false, unfolded: [] });
  const ids = (value: string) => value.split(',').map(decodeURIComponent);
  for (const part of hash.replace(/^#/, '').split('&')) {
    const eq = part.indexOf('=');
    if (eq < 0) continue;
    const key = part.slice(0, eq);
    const value = part.slice(eq + 1);
    if (key === 'src' && (SOURCES as readonly string[]).includes(value)) state.src = value as Source;
    else if (key === 'level' && (LEVELS as readonly string[]).includes(value)) state.level = value as Level;
    else if (key === 'tests') state.tests = value === '1';
    else if (key === 'open' && value !== '') state.open = value.split(',').map(decodeURIComponent);
    else if (key === 'focus' && value !== '') focus().nodes = ids(value);
    else if (key === 'fchanges' && value === '1') focus().changes = true;
    else if (key === 'fnear' && value === '1') focus().neighbors = true;
    else if (key === 'unfold' && value !== '') focus().unfolded = ids(value);
    else if (key === 'kinds') edges.kinds = decodeKinds(value) ?? edges.kinds;
    else {
      const flag = FLAGS.find(([k]) => k === key);
      if (flag) edges[flag[1]] = value === '1';
    }
  }
  return state;
}
