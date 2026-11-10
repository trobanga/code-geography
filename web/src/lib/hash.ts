import { LEVELS, type Level } from './types';

export interface HashState {
  level: Level;
  tests: boolean;
  open: string[];
}

export function encodeHash(s: HashState): string {
  const parts = [`level=${s.level}`, `tests=${s.tests ? 1 : 0}`];
  if (s.open.length > 0) parts.push(`open=${s.open.map(encodeURIComponent).join(',')}`);
  return '#' + parts.join('&');
}

export function decodeHash(hash: string): HashState {
  const state: HashState = { level: 'modules', tests: false, open: [] };
  for (const part of hash.replace(/^#/, '').split('&')) {
    const eq = part.indexOf('=');
    if (eq < 0) continue;
    const key = part.slice(0, eq);
    const value = part.slice(eq + 1);
    if (key === 'level' && (LEVELS as readonly string[]).includes(value)) state.level = value as Level;
    else if (key === 'tests') state.tests = value === '1';
    else if (key === 'open' && value !== '') state.open = value.split(',').map(decodeURIComponent);
  }
  return state;
}
