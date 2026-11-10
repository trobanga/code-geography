import { describe, expect, it } from 'vitest';
import { decodeHash, encodeHash } from './hash';

describe('hash', () => {
  it('round-trips level, tests and open ids with special characters', () => {
    const state = {
      level: 'types' as const,
      tests: true,
      open: ['type:a.B', 'fn:a.B#c(int, String)', 'x,y&z=1'],
    };
    expect(decodeHash(encodeHash(state))).toEqual(state);
  });

  it('decodes an empty or garbage hash to the start state', () => {
    const start = { level: 'modules', tests: false, open: [] };
    expect(decodeHash('')).toEqual(start);
    expect(decodeHash('#level=bogus&tests=7')).toEqual(start);
  });

  it('accepts a hash with leading # and missing fields', () => {
    expect(decodeHash('#level=packages')).toEqual({ level: 'packages', tests: false, open: [] });
  });
});
