import { describe, expect, it } from 'vitest';
import { filterBranches, filterCommits, rangeOf, shaOf } from './menuSearch';
import type { CommitRow } from './forgeDiff';

const row = (sha: string, title: string, author = 'Ada', day = '2026-11-01'): CommitRow => ({ sha, short: sha.slice(0, 7), title, author, day, current: false });

const ROWS = [
  row('a1b2c3d4'.padEnd(40, '0'), 'Fix the cut of a folded container'),
  row('ffee0011'.padEnd(40, '0'), 'Add the branch menu', 'Grace', '2026-10-30'),
  row('0123abcd'.padEnd(40, '0'), 'Fold the branch list'),
];

const titles = (q: string) => filterCommits(ROWS, q).map((r) => r.title);

describe('filterCommits', () => {
  it('keeps the rows that hold each word of the query in the title, the author or the day, in any case', () => {
    expect(titles('BRANCH')).toEqual(['Add the branch menu', 'Fold the branch list']);
    expect(titles('branch fold')).toEqual(['Fold the branch list']);
    expect(titles('grace')).toEqual(['Add the branch menu']);
    expect(titles('2026-10')).toEqual(['Add the branch menu']);
    expect(titles('  ')).toEqual(ROWS.map((r) => r.title));
    expect(titles('nothing')).toEqual([]);
  });

  it('finds a commit by the start of its sha, also together with other words', () => {
    expect(titles('A1B2')).toEqual(['Fix the cut of a folded container']);
    expect(titles('0123abc branch')).toEqual(['Fold the branch list']);
    // Not in the middle of the sha.
    expect(titles('b2c3')).toEqual([]);
  });
});

describe('shaOf', () => {
  it('takes a query of 7 to 40 hex digits as a sha, in lower case; anything else is none', () => {
    expect(shaOf(' A1B2C3D ')).toBe('a1b2c3d');
    expect(shaOf('f'.repeat(40))).toBe('f'.repeat(40));
    expect(shaOf('a1b2c3')).toBeNull();
    expect(shaOf('f'.repeat(41))).toBeNull();
    expect(shaOf('decade fix')).toBeNull();
    expect(shaOf('branchy')).toBeNull();
  });
});

describe('rangeOf', () => {
  it('takes A..B with two shas of 7 to 40 hex digits as a range, trimmed and in lower case', () => {
    expect(rangeOf(' A1B2C3D..' + 'F'.repeat(40) + ' ')).toEqual({ base: 'a1b2c3d', head: 'f'.repeat(40) });
  });

  it('gives none for A...B, a short sha, a single sha or anything else', () => {
    expect(rangeOf('a1b2c3d...ffee0011')).toBeNull();
    expect(rangeOf('a1b2c3..ffee0011')).toBeNull();
    expect(rangeOf('a1b2c3d..')).toBeNull();
    expect(rangeOf('a1b2c3d')).toBeNull();
    expect(rangeOf('main..a1b2c3d')).toBeNull();
  });
});

describe('filterBranches', () => {
  const names = (list: string[], q: string) => filterBranches(list.map((name) => ({ name, sha: name })), q).map((b) => b.name);

  it('gives the branches in alphabetical order, capitals not apart', () => {
    expect(names(['main', 'Zeta', 'feature/b', 'alpha', 'Feature/a'], ' ')).toEqual(['alpha', 'Feature/a', 'feature/b', 'main', 'Zeta']);
  });

  it('keeps the names that hold the characters of the query in this order, with gaps or not; first the names that hold the query as one block', () => {
    const list = ['feature/token-login', 'fix/tooltip-gap', 'main', 'login', 'release/log'];
    expect(names(list, 'LOG')).toEqual(['feature/token-login', 'login', 'release/log']);
    // "bet-on-a" holds "beta" with gaps only, so it goes after the two names with the block.
    expect(names(['bet-on-a', 'beta', 'alpha-beta'], 'beta')).toEqual(['alpha-beta', 'beta', 'bet-on-a']);
    expect(names(list, 'ftlg')).toEqual(['feature/token-login', 'fix/tooltip-gap']);
    expect(names(list, 'mainx')).toEqual([]);
  });
});
