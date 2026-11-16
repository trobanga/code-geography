import { describe, expect, it } from 'vitest';
import type { CommitSource, CommitText, PullRequest } from './forge';
import { commitBody, commitContext, commitView, contextCache, issueRefs, webBase, type ContextState } from './commitContext';

const SHA = '0123456789abcdef0123456789abcdef01234567';
const commitOf = (message: string, sha = SHA): CommitText => ({ sha, parents: [], title: message.split('\n', 1)[0], message, author: 'Ann', date: '' });
const pullOf = (number: number, body = '', title = `PR ${number}`): PullRequest => ({ number, title, state: 'merged', url: `https://github.com/a/b/pull/${number}`, body });

const gh = { owner: 'a', name: 'b' };
const fj = { owner: 'a', name: 'b', origin: 'https://codeberg.org', kind: 'forgejo' as const };

describe('issueRefs', () => {
  it('reads #12 as an issue of the same repository', () => {
    expect(issueRefs('Fixes #12 and more', gh)).toEqual([{ owner: 'a', name: 'b', n: 12, label: '#12', url: 'https://github.com/a/b/issues/12' }]);
  });

  it('reads owner/repo#3 with the label and the URL of that repository', () => {
    expect(issueRefs('see x/y#3', gh)).toEqual([{ owner: 'x', name: 'y', n: 3, label: 'x/y#3', url: 'https://github.com/x/y/issues/3' }]);
    expect(issueRefs('see x/y#3', fj)[0].url).toBe('https://codeberg.org/x/y/issues/3');
    expect(issueRefs('a/b#4', gh)[0].label).toBe('#4');
  });

  it('lists each reference once, in order of first appearance', () => {
    expect(issueRefs('#2 then #1 then #2', gh).map((r) => r.n)).toEqual([2, 1]);
    expect(issueRefs('#2 and a/b#2', gh).map((r) => r.n)).toEqual([2]);
  });

  it('ignores a # after a word character, /, & or ., and a number that goes on', () => {
    expect(issueRefs('a#1 &#39; x/#3 v1.#4 #12a #5_', gh)).toEqual([]);
    expect(issueRefs('(#7), #8.', gh).map((r) => r.n)).toEqual([7, 8]);
  });

  it('reads issue and pull URLs of the same host', () => {
    const t = 'https://github.com/a/b/issues/5 https://github.com/x/y/pull/6/files https://github.com/a/b/pulls/9';
    expect(issueRefs(t, gh)).toEqual([
      { owner: 'a', name: 'b', n: 5, label: '#5', url: 'https://github.com/a/b/issues/5' },
      { owner: 'x', name: 'y', n: 6, label: 'x/y#6', url: 'https://github.com/x/y/issues/6' },
    ]);
    const f = 'https://codeberg.org/a/b/pulls/9 https://codeberg.org/a/b/pull/10';
    expect(issueRefs(f, fj).map((r) => r.n)).toEqual([9]);
  });

  it('ignores a URL of another host', () => {
    expect(issueRefs('https://gitlab.com/a/b/issues/5', gh)).toEqual([]);
    expect(issueRefs('https://github.com/a/b/issues/5', fj)).toEqual([]);
  });
});

describe('webBase', () => {
  it('is github.com for GitHub, the origin for a Forgejo host', () => {
    expect(webBase({ owner: 'a', name: 'b' })).toBe('https://github.com/a/b');
    expect(webBase({ owner: 'a', name: 'b', origin: 'https://codeberg.org', kind: 'forgejo' })).toBe('https://codeberg.org/a/b');
  });
});

describe('commitBody', () => {
  it('is the message without its first line, trimmed', () => {
    expect(commitBody('title\n\nbody line\nmore\n')).toBe('body line\nmore');
    expect(commitBody('title')).toBe('');
    expect(commitBody('title\n')).toBe('');
  });
});

describe('commitView', () => {
  it('has the parts of the commit for display', () => {
    const v = commitView(gh, commitOf('Fix it (#7)\n\nCloses #3 and x/y#4'), [pullOf(7, 'Also see #5 and #7')]);
    expect(v).toMatchObject({ sha: SHA, short: SHA.slice(0, 7), title: 'Fix it (#7)', body: 'Closes #3 and x/y#4', url: 'https://github.com/a/b/commit/' + SHA });
    expect(v.pulls).toEqual([{ ...pullOf(7, 'Also see #5 and #7'), label: '#7' }]);
    expect(v.refs.map((r) => r.label)).toEqual(['#3', 'x/y#4', '#5']);
  });

  it('links a pull request to its page on the forge, not to the URL of the answer', () => {
    const evil = { ...pullOf(7), url: 'javascript:alert(1)' };
    expect(commitView(gh, commitOf('x'), [evil]).pulls[0].url).toBe('https://github.com/a/b/pull/7');
    expect(commitView(fj, commitOf('x'), [evil]).pulls[0].url).toBe('https://codeberg.org/a/b/pulls/7');
  });

  it('is empty for a commit without pull request and reference', () => {
    const v = commitView(gh, commitOf('plain'), []);
    expect([v.pulls, v.refs, v.body]).toEqual([[], [], '']);
  });
});

const C1 = '1'.repeat(40);
const C2 = '2'.repeat(40);

/** A fake source: lists and pull requests by answer, records each call; `fail` makes a call reject. */
function setup(opts: { commits?: CommitText[]; total?: number; pulls?: Record<string, PullRequest[]>; cache?: ReturnType<typeof contextCache> } = {}) {
  const calls: string[] = [];
  const fail = new Set<string>();
  /** Pull request answers held back until released, by sha. */
  const held = new Map<string, () => void>();
  const hold = new Set<string>();
  const commits = opts.commits ?? [commitOf('only one', C1)];
  const source: CommitSource = {
    async between(_repo, base, head) {
      calls.push(`between ${base}..${head}`);
      if (fail.has('between')) throw new Error('HTTP 500');
      return { commits, total: opts.total ?? commits.length };
    },
    async pullsOf(_repo, sha) {
      calls.push(`pullsOf ${sha[0]}`);
      if (hold.has(sha)) await new Promise<void>((ok) => held.set(sha, ok));
      if (fail.has('pullsOf')) throw new Error('HTTP 500');
      return opts.pulls?.[sha] ?? [];
    },
  };
  const states: ContextState[] = [];
  const cache = opts.cache ?? contextCache();
  const make = () => commitContext({ source, repo: gh, range: { base: 'main', head: 'feat' }, cache, onState: (s) => states.push(s) });
  return { ctx: make(), make, calls, fail, hold, held, states, cache, last: () => states[states.length - 1] };
}

describe('commitContext', () => {
  it('asks for nothing before the first toggle; a toggle opens and loads the list once', async () => {
    const t = setup({ commits: [commitOf('one', C1), commitOf('two', C2)] });
    expect(t.calls).toEqual([]);
    await t.ctx.toggle();
    expect(t.calls).toEqual(['between main..feat']);
    expect(t.last()).toMatchObject({ open: true, status: null, total: 2, current: null });
    expect(t.last().commits?.map((c) => c.sha)).toEqual([C1, C2]);
    await t.ctx.toggle();
    expect(t.last().open).toBe(false);
    await t.ctx.toggle();
    expect(t.last().open).toBe(true);
    expect(t.calls).toEqual(['between main..feat']);
  });

  it('opens a single commit by itself and loads its pull requests', async () => {
    const t = setup({ pulls: { [C1]: [pullOf(7)] } });
    await t.ctx.toggle();
    expect(t.last()).toMatchObject({ current: C1, pulls: [pullOf(7)], pullStatus: null });
    expect(t.calls).toEqual(['between main..feat', 'pullsOf 1']);
  });

  it('opens a commit of a longer list on request, and closes it on a second request', async () => {
    const t = setup({ commits: [commitOf('one', C1), commitOf('two', C2)], pulls: { [C2]: [pullOf(8)] } });
    await t.ctx.toggle();
    expect(t.last().current).toBeNull();
    await t.ctx.openCommit(C2);
    expect(t.last()).toMatchObject({ current: C2, pulls: [pullOf(8)] });
    await t.ctx.openCommit(C2);
    expect(t.last()).toMatchObject({ current: null, pulls: null, pullStatus: null });
  });

  it('asks once for the pull requests of a sha, also through a second context on the same cache', async () => {
    const two = [commitOf('one', C1), commitOf('two', C2)];
    const t = setup({ commits: two, pulls: { [C1]: [pullOf(1)] } });
    await t.ctx.toggle();
    await t.ctx.openCommit(C1);
    await t.ctx.openCommit(C1);
    await t.ctx.openCommit(C1);
    const again = setup({ commits: two, cache: t.cache });
    await again.ctx.toggle();
    await again.ctx.openCommit(C1);
    expect(t.calls).toEqual(['between main..feat', 'pullsOf 1']);
    expect(again.calls).toEqual([]);
    expect(again.last().pulls).toEqual([pullOf(1)]);
  });

  it('shows a failed list and tries again on the next open', async () => {
    const t = setup();
    t.fail.add('between');
    await t.ctx.toggle();
    expect(t.last().status).toBe('cannot list the commits: HTTP 500');
    expect(t.last().commits).toBeNull();
    t.fail.clear();
    await t.ctx.toggle();
    await t.ctx.toggle();
    expect(t.last()).toMatchObject({ status: null, current: C1 });
    expect(t.calls.filter((c) => c.startsWith('between')).length).toBe(2);
  });

  it('shows a failed pull request request and keeps the list and the open commit', async () => {
    const t = setup({ commits: [commitOf('one', C1), commitOf('two', C2)] });
    await t.ctx.toggle();
    t.fail.add('pullsOf');
    await t.ctx.openCommit(C1);
    expect(t.last()).toMatchObject({ pullStatus: 'cannot list the pull requests: HTTP 500', pulls: null, current: C1 });
    expect(t.last().commits?.length).toBe(2);
  });

  it('drops a pull request answer for a commit that is no longer open', async () => {
    const t = setup({ commits: [commitOf('one', C1), commitOf('two', C2)], pulls: { [C1]: [pullOf(1)], [C2]: [pullOf(2)] } });
    await t.ctx.toggle();
    t.hold.add(C1);
    const slow = t.ctx.openCommit(C1);
    await t.ctx.openCommit(C2);
    t.hold.delete(C1);
    t.held.get(C1)!();
    await slow;
    expect(t.last()).toMatchObject({ current: C2, pulls: [pullOf(2)] });
  });

  it('asks once for the pull requests of a commit that opens again while its answer is pending', async () => {
    const t = setup({ commits: [commitOf('one', C1), commitOf('two', C2)], pulls: { [C1]: [pullOf(1)], [C2]: [pullOf(2)] } });
    await t.ctx.toggle();
    t.hold.add(C1);
    const first = t.ctx.openCommit(C1);
    await t.ctx.openCommit(C2);
    const again = t.ctx.openCommit(C1);
    t.held.get(C1)!();
    await Promise.all([first, again]);
    expect(t.calls.filter((c) => c === 'pullsOf 1')).toEqual(['pullsOf 1']);
    expect(t.last()).toMatchObject({ current: C1, pulls: [pullOf(1)], pullStatus: null });
  });
});
