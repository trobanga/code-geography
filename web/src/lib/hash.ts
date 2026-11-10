import { DEFAULT_EDGE_OPTIONS, MIN_COUNT_MAX, type EdgeOptions } from './edgeOptions';
import { EDGE_KINDS, LEVELS, type EdgeKind, type Level } from './types';

export interface HashState {
  level: Level;
  tests: boolean;
  open: string[];
  edges: EdgeOptions;
}

const FLAGS = [
  ['sel', 'onSelection'],
  ['lift', 'lift'],
  ['merge', 'mergeImports'],
  ['reduce', 'reduce'],
] as const;

export function encodeHash(s: HashState): string {
  const parts = [`level=${s.level}`, `tests=${s.tests ? 1 : 0}`];
  if (s.open.length > 0) parts.push(`open=${s.open.map(encodeURIComponent).join(',')}`);
  const on = EDGE_KINDS.filter((k) => s.edges.kinds[k]);
  if (on.length < EDGE_KINDS.length) parts.push(`kinds=${on.length > 0 ? on.join(',') : 'none'}`);
  if (s.edges.min !== DEFAULT_EDGE_OPTIONS.min) parts.push(`min=${s.edges.min}`);
  for (const [key, field] of FLAGS) if (s.edges[field]) parts.push(`${key}=1`);
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
  for (const part of hash.replace(/^#/, '').split('&')) {
    const eq = part.indexOf('=');
    if (eq < 0) continue;
    const key = part.slice(0, eq);
    const value = part.slice(eq + 1);
    if (key === 'level' && (LEVELS as readonly string[]).includes(value)) state.level = value as Level;
    else if (key === 'tests') state.tests = value === '1';
    else if (key === 'open' && value !== '') state.open = value.split(',').map(decodeURIComponent);
    else if (key === 'kinds') edges.kinds = decodeKinds(value) ?? edges.kinds;
    else if (key === 'min' && /^\d+$/.test(value)) edges.min = Math.min(MIN_COUNT_MAX, Math.max(1, Number(value)));
    else {
      const flag = FLAGS.find(([k]) => k === key);
      if (flag) edges[flag[1]] = value === '1';
    }
  }
  return state;
}
