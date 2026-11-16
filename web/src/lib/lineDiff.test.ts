import { describe, expect, it } from 'vitest';
import { lineDiff } from './lineDiff';

const lines = (text: string): string[] => (text === '' ? [] : text.split(' '));
const diff = (base: string, head: string, maxEdits?: number) => lineDiff(lines(base), lines(head), maxEdits);
const both = (n: number) => ({ base: n, head: n });

describe('lineDiff', () => {
  it('pairs the lines of equal texts', () => {
    expect(diff('a b c', 'a b c')).toEqual([both(1), both(2), both(3)]);
  });

  it('shows an added line between the lines around it', () => {
    expect(diff('a c', 'a b c')).toEqual([both(1), { base: null, head: 2 }, { base: 2, head: 3 }]);
  });

  it('shows a changed line as the removed line followed by the added one', () => {
    expect(diff('a b c', 'a x c')).toEqual([both(1), { base: 2, head: null }, { base: null, head: 2 }, both(3)]);
  });

  it('puts the removed lines of a run of changes before the added ones', () => {
    expect(diff('a b c d', 'a x y d')).toEqual([
      both(1),
      { base: 2, head: null },
      { base: 3, head: null },
      { base: null, head: 2 },
      { base: null, head: 3 },
      both(4),
    ]);
  });

  it('shows all lines added for an empty base, all removed for an empty head, and nothing for both', () => {
    expect(diff('', 'a b')).toEqual([{ base: null, head: 1 }, { base: null, head: 2 }]);
    expect(diff('a b', '')).toEqual([{ base: 1, head: null }, { base: 2, head: null }]);
    expect(diff('', '')).toEqual([]);
  });

  it('handles changes at the very start and the very end', () => {
    expect(diff('a b c', 'x b c')).toEqual([{ base: 1, head: null }, { base: null, head: 1 }, both(2), both(3)]);
    expect(diff('a b c', 'a b x')).toEqual([both(1), both(2), { base: 3, head: null }, { base: null, head: 3 }]);
    expect(diff('a b', 'a b c')).toEqual([both(1), both(2), { base: null, head: 3 }]);
    expect(diff('b c', 'a b c')).toEqual([{ base: null, head: 1 }, { base: 1, head: 2 }, { base: 2, head: 3 }]);
  });

  it('finds a shortest script: it keeps a longest common subsequence', () => {
    const base = lines('a b c a b b a');
    const head = lines('c b a b a c');

    const result = lineDiff(base, head);

    const kept = result.filter((l) => l.base !== null && l.head !== null);
    expect(kept).toHaveLength(4);
    expect(result.filter((l) => l.head === null)).toHaveLength(3);
    expect(result.filter((l) => l.base === null)).toHaveLength(2);
    for (const l of kept) expect(base[l.base! - 1]).toBe(head[l.head! - 1]);
    // Each line of both sides once, in the order of the files.
    expect(result.filter((l) => l.base !== null).map((l) => l.base)).toEqual([1, 2, 3, 4, 5, 6, 7]);
    expect(result.filter((l) => l.head !== null).map((l) => l.head)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it('replaces the part between the common start and end when it needs more than maxEdits edits', () => {
    const base = 'a b c d e f g';
    const head = 'a x c d e y g';

    expect(diff(base, head)).toEqual([
      both(1),
      { base: 2, head: null },
      { base: null, head: 2 },
      both(3),
      both(4),
      both(5),
      { base: 6, head: null },
      { base: null, head: 6 },
      both(7),
    ]);
    expect(diff(base, head, 1)).toEqual([
      both(1),
      ...[2, 3, 4, 5, 6].map((n) => ({ base: n, head: null })),
      ...[2, 3, 4, 5, 6].map((n) => ({ base: null, head: n })),
      both(7),
    ]);
    // Four edits are within reach of maxEdits = 4, and out of reach of 3.
    expect(diff(base, head, 4)).toEqual(diff(base, head));
    expect(diff(base, head, 3)).toEqual(diff(base, head, 1));
  });

  it('counts a repeated line once when the common start and the common end overlap', () => {
    expect(diff('a a', 'a')).toEqual([both(1), { base: 2, head: null }]);
    expect(diff('a', 'a a')).toEqual([both(1), { base: null, head: 2 }]);
    expect(diff('a b c', 'a b')).toEqual([both(1), both(2), { base: 3, head: null }]);
    expect(diff('a a a a', 'a a')).toEqual([both(1), both(2), { base: 3, head: null }, { base: 4, head: null }]);
  });

  it('replaces two texts with nothing in common above the default limit, and keeps a large change below it', () => {
    const numbered = (prefix: string, n: number) => Array.from({ length: n }, (_, i) => `${prefix}${i}`);

    // 1200 edits: above the limit of 1000.
    const apart = lineDiff(numbered('a', 600), numbered('b', 600));
    expect(apart).toHaveLength(1200);
    expect(apart.slice(0, 600).every((l, i) => l.base === i + 1 && l.head === null)).toBe(true);
    expect(apart.slice(600).every((l, i) => l.base === null && l.head === i + 1)).toBe(true);

    // Every fifth line of 2000 changed: 800 edits, below the limit, so the other lines stay paired.
    const base = numbered('line', 2000);
    const head = base.map((l, i) => (i % 5 === 0 ? `changed${i}` : l));
    const within = lineDiff(base, head);
    expect(within.filter((l) => l.base !== null && l.head !== null)).toHaveLength(1600);
    expect(within.filter((l) => l.base !== null && l.head !== null).every((l) => l.base === l.head)).toBe(true);
    expect(within.filter((l) => l.head === null)).toHaveLength(400);
    expect(within.filter((l) => l.base === null)).toHaveLength(400);
  });

  it('shows a removed line and shifts the numbers of the lines after it', () => {
    expect(diff('a b c', 'a c')).toEqual([both(1), { base: 2, head: null }, { base: 3, head: 2 }]);
  });
});
