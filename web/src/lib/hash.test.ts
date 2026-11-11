import { describe, expect, it } from 'vitest';
import { DEFAULT_EDGE_OPTIONS } from './edgeOptions';
import { decodeHash, encodeHash, freshHash } from './hash';
import { NO_FOCUS } from './focus';

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
        kinds: { extends: true, implements: false, imports: false, type_ref: true, calls: true },
        lift: false,
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

  it('round-trips the focus with special characters', () => {
    const base = { level: 'types' as const, tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    const focus = { nodes: ['type:a.B', 'x,y&z=1'], changes: true, neighbors: true };
    expect(decodeHash(encodeHash({ ...base, focus })).focus).toEqual(focus);
    const changes = { nodes: [], changes: true, neighbors: false };
    expect(decodeHash(encodeHash({ ...base, focus: changes })).focus).toEqual(changes);
  });

  it('writes nothing for an empty focus and decodes none to no focus', () => {
    const base = { level: 'modules' as const, tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    expect(encodeHash({ ...base, focus: NO_FOCUS })).toBe('#level=modules&tests=0');
    expect(decodeHash('#level=modules').focus).toBeUndefined();
  });

  it('round-trips all kinds off', () => {
    const edges = { ...DEFAULT_EDGE_OPTIONS, kinds: { extends: false, implements: false, imports: false, type_ref: false, calls: false } };
    expect(decodeHash(encodeHash({ level: 'modules', tests: false, open: [], edges })).edges).toEqual(edges);
  });

  it('writes nothing for edge options at their defaults', () => {
    expect(encodeHash({ level: 'modules', tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS })).toBe('#level=modules&tests=0');
  });

  it('decodes an empty or garbage hash to the start state', () => {
    const start = { level: 'modules', tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    expect(decodeHash('')).toEqual(start);
    expect(decodeHash('#level=bogus&tests=7&min=99&kinds=bogus')).toEqual(start);
  });

  it('writes a default-on option only when it is off', () => {
    const base = { level: 'modules' as const, tests: false, open: [] };
    const off = { ...DEFAULT_EDGE_OPTIONS, lift: false };
    expect(DEFAULT_EDGE_OPTIONS.lift).toBe(true);
    expect(encodeHash({ ...base, edges: off })).toBe('#level=modules&tests=0&lift=0');
    expect(decodeHash(encodeHash({ ...base, edges: off })).edges).toEqual(off);
  });

  it('round-trips the unfolded containers without a focus, and reads them from an old focus hash', () => {
    const base = { level: 'packages' as const, tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    const s = decodeHash(encodeHash({ ...base, unfold: ['pkg:a', 'mod:x,y'] }));
    expect(s.unfold).toEqual(['pkg:a', 'mod:x,y']);
    expect(s.focus).toBeUndefined();
    const old = decodeHash('#level=types&focus=type%3Aa.B&unfold=pkg%3Aa');
    expect(old.unfold).toEqual(['pkg:a']);
    expect(old.focus).toEqual({ nodes: ['type:a.B'], changes: false, neighbors: false });
  });

  it('loads an old hash with removed keys', () => {
    expect(decodeHash('#level=packages&min=3&reduce=1&merge=0&lift=1').edges).toEqual(DEFAULT_EDGE_OPTIONS);
  });

  it('keeps a GitHub source readable as gh=owner/repo@sha, first', () => {
    const state = { gh: 'a/b.c@0123456789abcdef0123456789abcdef01234567', level: 'modules' as const, tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS };
    const h = encodeHash(state);
    expect(h).toBe('#gh=a/b.c@0123456789abcdef0123456789abcdef01234567&level=modules&tests=0');
    expect(decodeHash(h)).toEqual(state);
  });

  it('accepts a hash with leading # and missing fields', () => {
    expect(decodeHash('#level=packages')).toEqual({ level: 'packages', tests: false, open: [], edges: DEFAULT_EDGE_OPTIONS });
  });
});

describe('freshHash', () => {
  it('drops source, level, open ids and focus of the old graph and keeps tests and edge options', () => {
    const fresh = decodeHash(freshHash('#src=commit&level=types&tests=1&open=pkg::a&kinds=calls&focus=type:a.B'));
    expect(fresh).toEqual({
      level: 'modules',
      tests: true,
      open: [],
      edges: { ...DEFAULT_EDGE_OPTIONS, kinds: { extends: false, implements: false, imports: false, type_ref: false, calls: true } },
    });
  });
});
