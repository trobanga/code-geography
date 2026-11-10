import { describe, expect, it } from 'vitest';
import { DEFAULT_EDGE_OPTIONS } from './edgeOptions';
import { decodeHash, encodeHash } from './hash';

describe('hash', () => {
  it('round-trips level, tests and open ids with special characters', () => {
    const state = {
      level: 'types' as const,
      tests: true,
      open: ['type:a.B', 'fn:a.B#c(int, String)', 'x,y&z=1'],
      edges: DEFAULT_EDGE_OPTIONS,
    };
    expect(decodeHash(encodeHash(state))).toEqual(state);
  });

  it('round-trips every edge option', () => {
    const state = {
      level: 'packages' as const,
      tests: false,
      open: [],
      edges: {
        onSelection: true,
        kinds: { extends: true, implements: false, imports: false, type_ref: true },
        min: 3,
        lift: true,
        mergeImports: true,
        reduce: true,
        changesOnly: true,
      },
    };
    expect(decodeHash(encodeHash(state))).toEqual(state);
  });

  it('round-trips the source and leaves it out when not given', () => {
    const base = { level: 'modules' as const, tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    expect(decodeHash(encodeHash({ ...base, src: 'commit' })).src).toBe('commit');
    expect(decodeHash(encodeHash({ ...base, src: 'graph' })).src).toBe('graph');
    expect(decodeHash(encodeHash(base)).src).toBeUndefined();
    expect(decodeHash('#src=bogus').src).toBeUndefined();
  });

  it('round-trips all kinds off', () => {
    const edges = { ...DEFAULT_EDGE_OPTIONS, kinds: { extends: false, implements: false, imports: false, type_ref: false } };
    expect(decodeHash(encodeHash({ level: 'modules', tests: false, open: [], edges })).edges).toEqual(edges);
  });

  it('writes nothing for edge options at their defaults', () => {
    expect(encodeHash({ level: 'modules', tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS })).toBe('#level=modules&tests=0');
  });

  it('decodes an empty or garbage hash to the start state', () => {
    const start = { level: 'modules', tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    expect(decodeHash('')).toEqual(start);
    expect(decodeHash('#level=bogus&tests=7&min=99&kinds=bogus')).toEqual({ ...start, edges: { ...DEFAULT_EDGE_OPTIONS, min: 10 } });
  });

  it('accepts a hash with leading # and missing fields', () => {
    expect(decodeHash('#level=packages')).toEqual({ level: 'packages', tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS });
  });
});
