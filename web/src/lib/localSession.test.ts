import { describe, expect, it, vi } from 'vitest';
import type { Commit } from './forge';
import type { Range } from './forgeDiff';
import { localSession, NO_LOCAL, type LocalRepo, type LocalState } from './localSession';
import type { Summary } from './project';
import type { SourceTexts } from './source';

const report = { calls: { to_function: 0, to_type: 0, external: 0, unresolved: 0 } };
const sha = (i: number) => String(i).padStart(3, '0').padEnd(40, 'f');

/** A linear history of `length` commits: `sha(0)` is the newest, each has the next one as parent; the last is a root commit. */
function history(length: number): Commit[] {
  return Array.from({ length }, (_, i) => ({
    sha: sha(i),
    parents: i + 1 < length ? [sha(i + 1)] : [],
    title: `commit ${i}`,
    author: 'A',
    date: '2026-11-01T00:00:00Z',
  }));
}

/** A fake handle on the worker of a repository, a state and the calls that the session makes of the app. */
function setup(commits: Commit[] = history(70)) {
  const calls: string[] = [];
  const phases: (string | null)[] = [];
  const errors: string[] = [];
  const shown: { text: string; name: string; summary: Summary; texts: SourceTexts }[] = [];
  /** The heads whose `overlay` waits until released: for stale replies. */
  const holds = new Set<string>();
  const held = new Map<string, () => void>();
  let closed = false;
  const find = (from: string) => commits.findIndex((c) => c.sha.startsWith(from));
  const repo: LocalRepo = {
    async log(from, n) {
      calls.push(`log ${from === null ? 'HEAD' : from.slice(0, 3)} ${n}`);
      const at = from === null ? 0 : find(from);
      if (at < 0) throw new Error(`no ref ${from}`);
      return commits.slice(at, at + n);
    },
    async overlay(base, head, rootName) {
      calls.push(`overlay ${base?.slice(0, 3) ?? 'none'}..${head.slice(0, 3)}`);
      if (holds.has(head)) await new Promise<void>((ok) => held.set(head, ok));
      const diff = { base: base?.slice(0, 7) ?? 'none', head: head.slice(0, 7), files: [{ path: 'A.java', change: 'modified' }] };
      const nodes = [{ id: 'A', parent: null, kind: 'type', name: 'A', change: 'modified' }];
      return { graphText: JSON.stringify({ nodes, edges: [], diff }), report, base: { 'A.java': `base ${rootName}` }, head: { 'A.java': `head ${head.slice(0, 3)}` }, indexes: 2 };
    },
    close: () => void (closed = true),
  };
  let seq = 0;
  let state: LocalState = NO_LOCAL;
  const worktree = {
    text: '{"nodes":[],"edges":[]}',
    summary: { name: 'repo', sources: 1, nodes: 0, edges: 0, resolved: 0, unresolved: 0, text: 'repo: 1 source files', tooltip: '' } as Summary,
    texts: { head: new Map([['A.java', 'wt']]), base: new Map([['A.java', 'head']]) } as SourceTexts,
  };
  const session = localSession({
    begin: () => {
      const my = ++seq;
      return () => my === seq;
    },
    phase: (p) => phases.push(p),
    fail: (m) => errors.push(m),
    show: async (text, summary, texts) => void shown.push({ text, name: summary.name, summary, texts }),
    onState: (s) => (state = s),
  });
  session.start(repo, 'repo', worktree);
  return { session, repo, calls, phases, errors, shown, holds, held, worktree, state: () => state, isClosed: () => closed };
}

/** Lets the pending promises run until `done`. */
async function until(done: () => boolean) {
  for (let i = 0; i < 200 && !done(); i++) await Promise.resolve();
}

describe('localSession', () => {
  it('starts with the working tree shown and the list closed', () => {
    const t = setup();

    expect(t.state()).toMatchObject({ active: true, shown: 'worktree', commits: null, listStatus: null, listOpen: false });
  });

  it('loads the first 30 commits when the list opens, and the sha that the next page starts at', async () => {
    const t = setup();

    t.session.toggleList();
    expect(t.state()).toMatchObject({ listOpen: true, listStatus: 'listing the last 30 commits' });
    await until(() => t.state().commits !== null);

    expect(t.calls).toEqual(['log HEAD 30']);
    expect(t.state().commits?.list.map((c) => c.sha)).toEqual(Array.from({ length: 30 }, (_, i) => sha(i)));
    expect(t.state().commits?.next).toBe(sha(30));
    expect(t.state().listStatus).toBeNull();
  });

  it('appends the next page from the sha that it starts at; the history ends with a short page', async () => {
    const t = setup(history(40));
    t.session.toggleList();
    await until(() => t.state().commits !== null);

    const more = t.session.loadMore();
    expect(t.state().listStatus).toBe('listing 30 more commits');
    await more;

    expect(t.calls).toEqual(['log HEAD 30', 'log 030 30']);
    expect(t.state().commits?.list.map((c) => c.sha)).toEqual(Array.from({ length: 40 }, (_, i) => sha(i)));
    expect(t.state().commits?.next).toBeNull();
    expect(t.state().listStatus).toBeNull();
    await t.session.loadMore();
    expect(t.calls).toHaveLength(2);
  });

  it('reads more pages while the commits of update bots leave fewer than 30 rows, as the forge list does', async () => {
    // Of 200 commits, every one but each fifth is by Renovate: a page of 30 gives 6 rows.
    const t = setup(history(200).map((c, i) => (i % 5 === 0 ? c : { ...c, author: 'renovate[bot]' })));

    t.session.toggleList();
    await until(() => t.state().commits !== null);

    expect(t.calls).toEqual(['log HEAD 30', 'log 030 30', 'log 060 30', 'log 090 30', 'log 120 30']);
    expect(t.state().commits?.list).toHaveLength(150);
    expect(t.state().commits?.next).toBe(sha(150));

    await t.session.loadMore();
    expect(t.calls.slice(5)).toEqual(['log 150 30', 'log 180 30']);
    expect(t.state().commits?.list).toHaveLength(200);
    expect(t.state().commits?.next).toBeNull();
  });

  it('knows the end of a history of one short page at once, and keeps the list when it closes and opens again', async () => {
    const t = setup(history(3));
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    expect(t.state().commits?.next).toBeNull();

    t.session.closeList();
    expect(t.state().listOpen).toBe(false);
    t.session.toggleList();

    expect(t.state()).toMatchObject({ listOpen: true, listStatus: null });
    expect(t.calls).toEqual(['log HEAD 30']);
  });

  it('says why the list failed, and asks again at the next open', async () => {
    const t = setup();
    const log = vi.spyOn(t.repo, 'log').mockRejectedValueOnce(new Error('bad object'));
    t.session.toggleList();
    await until(() => t.state().listStatus !== 'listing the last 30 commits');

    expect(t.state()).toMatchObject({ commits: null, listStatus: 'cannot list the commits: bad object' });
    t.session.toggleList();
    t.session.toggleList();
    await until(() => t.state().commits !== null);

    expect(log).toHaveBeenCalledTimes(2);
  });

  it('picks a commit: the overlay against its first parent, shown with the texts and the names of its sides', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);

    await t.session.pick(sha(2));

    expect(t.calls.slice(1)).toEqual([`overlay ${sha(3).slice(0, 3)}..${sha(2).slice(0, 3)}`]);
    expect(t.phases).toEqual([`indexing ${sha(3).slice(0, 7)} → ${sha(2).slice(0, 7)} of repo`, null]);
    expect(t.shown).toHaveLength(1);
    expect(t.shown[0].name).toBe(`repo ${sha(3).slice(0, 7)} → ${sha(2).slice(0, 7)}`);
    expect(t.shown[0].summary.text).toMatch(/, 1 changed files$/);
    expect(t.shown[0].summary.tooltip).toContain('a reload returns to the start screen');
    expect(JSON.parse(t.shown[0].text).diff).toMatchObject({ base: sha(3).slice(0, 7), head: sha(2).slice(0, 7) });
    expect(t.shown[0].texts).toEqual({
      head: new Map([['A.java', `head ${sha(2).slice(0, 3)}`]]),
      base: new Map([['A.java', 'base repo']]),
      names: { base: sha(3).slice(0, 7), head: sha(2).slice(0, 7) },
    });
    expect(t.state()).toMatchObject({ shown: { base: sha(3), head: sha(2) }, listOpen: false });
    expect(t.state().commits?.list).toHaveLength(30);
  });

  it('compares a merge commit with its first parent, and a root commit with nothing', async () => {
    const merge = { ...history(3)[0], parents: [sha(1), sha(9)] };
    const t = setup([merge, ...history(3).slice(1)]);
    t.session.toggleList();
    await until(() => t.state().commits !== null);

    await t.session.pick(sha(0));
    await t.session.pick(sha(2));

    expect(t.calls.slice(1)).toEqual([`overlay ${sha(1).slice(0, 3)}..${sha(0).slice(0, 3)}`, 'overlay none..002']);
  });

  it('does nothing when the commit picked is the one shown, but closes the list', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    await t.session.pick(sha(2));
    t.session.toggleList();

    await t.session.pick(sha(2));

    expect(t.calls).toHaveLength(2);
    expect(t.shown).toHaveLength(1);
    expect(t.state().listOpen).toBe(false);
  });

  it('shows the working tree again from memory, with the texts of the open', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    await t.session.pick(sha(2));
    const calls = t.calls.length;

    await t.session.pickWorktree();

    expect(t.calls).toHaveLength(calls);
    expect(t.shown).toHaveLength(2);
    expect(t.shown[1]).toMatchObject({ text: t.worktree.text, summary: t.worktree.summary, texts: t.worktree.texts });
    expect(t.state().shown).toBe('worktree');
    expect(t.state().listOpen).toBe(false);
  });

  it('ignores the reply of an overlay that a later choice replaced', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    t.holds.add(sha(2));

    const slow = t.session.pick(sha(2));
    await until(() => t.held.has(sha(2)));
    await t.session.pick(sha(1));
    t.held.get(sha(2))!();
    await slow;

    expect(t.shown.map((x) => x.name)).toEqual([`repo ${sha(2).slice(0, 7)} → ${sha(1).slice(0, 7)}`]);
    expect(t.state().shown).toMatchObject({ head: sha(1) });
    expect(t.phases.at(-1)).toBeNull();
    expect(t.errors).toEqual([]);
  });

  it('drops a pending overlay when the commit or the working tree shown is picked again', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    await t.session.pick(sha(1));
    t.holds.add(sha(2));

    // The commit shown is picked again while another one is built: the other one is stale.
    const slow = t.session.pick(sha(2));
    await until(() => t.held.has(sha(2)));
    await t.session.pick(sha(1));
    t.held.get(sha(2))!();
    await slow;
    expect(t.shown).toHaveLength(1);
    expect(t.state().shown).toMatchObject({ head: sha(1) });
    expect(t.phases.at(-1)).toBeNull();

    // The same for the working tree.
    await t.session.pickWorktree();
    t.holds.add(sha(3));
    t.held.clear();
    const slower = t.session.pick(sha(3));
    await until(() => t.held.has(sha(3)));
    await t.session.pickWorktree();
    t.held.get(sha(3))!();
    await slower;
    expect(t.state().shown).toBe('worktree');
    expect(t.shown).toHaveLength(2);
    expect(t.phases.at(-1)).toBeNull();
  });

  it('keeps the picture and says why when the overlay fails', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    vi.spyOn(t.repo, 'overlay').mockRejectedValueOnce(new Error('bad object'));
    const before = t.state();

    await t.session.pick(sha(2));

    expect(t.errors).toEqual(['cannot build the overlay of repo: bad object']);
    expect(t.shown).toHaveLength(0);
    expect(t.state().shown).toBe(before.shown);
    expect(t.phases.at(-1)).toBeNull();
  });

  it('picks a listed commit by the start of its sha without asking the worker for it', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);

    await t.session.pickSha(sha(5).slice(0, 7));

    expect(t.calls.slice(1)).toEqual([`overlay ${sha(6).slice(0, 3)}..${sha(5).slice(0, 3)}`]);
  });

  it('resolves a sha that is not listed with a log of one commit, then compares it with its first parent', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);

    await t.session.pickSha(sha(40).slice(0, 7));

    expect(t.calls.slice(1)).toEqual([`log ${sha(40).slice(0, 3)} 1`, `overlay ${sha(41).slice(0, 3)}..${sha(40).slice(0, 3)}`]);
    expect(t.state()).toMatchObject({ shown: { base: sha(41), head: sha(40) }, listOpen: false });
    expect(t.state().commits?.list).toHaveLength(30);
  });

  it('reports a sha that names no commit and leaves the picture', async () => {
    const t = setup();

    await t.session.pickSha('deadbee');

    expect(t.errors).toEqual(['cannot open deadbee: no ref deadbee']);
    expect(t.shown).toHaveLength(0);
    expect(t.state().shown).toBe('worktree');
    expect(t.phases.at(-1)).toBeNull();
  });

  it('opens a range A..B as the overlay of the tree of B against the tree of A, both resolved to full shas', async () => {
    const t = setup();

    await t.session.pickRange(sha(30).slice(0, 7), sha(10).slice(0, 7));

    expect(t.calls).toEqual([`log ${sha(30).slice(0, 3)} 1`, `log ${sha(10).slice(0, 3)} 1`, `overlay ${sha(30).slice(0, 3)}..${sha(10).slice(0, 3)}`]);
    expect(t.state().shown).toEqual({ base: sha(30), head: sha(10) });
    expect(t.shown[0].name).toBe(`repo ${sha(30).slice(0, 7)} → ${sha(10).slice(0, 7)}`);
  });

  it('reports a range with an unknown end and shows nothing', async () => {
    const t = setup();

    await t.session.pickRange(sha(30).slice(0, 7), 'deadbee');

    expect(t.errors).toEqual(['cannot open the range: no ref deadbee']);
    expect(t.shown).toHaveLength(0);
  });

  it('closes the handle on forget and returns to the inactive state; a pending overlay changes nothing', async () => {
    const t = setup();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    t.holds.add(sha(2));
    const slow = t.session.pick(sha(2));
    await until(() => t.held.has(sha(2)));

    t.session.forget();
    t.held.get(sha(2))!();
    await slow;

    expect(t.isClosed()).toBe(true);
    expect(t.state()).toEqual(NO_LOCAL);
    expect(t.shown).toHaveLength(0);
    t.session.toggleList();
    expect(t.state()).toEqual(NO_LOCAL);
  });

  it('closes the handle of an earlier repository when another one starts', () => {
    const t = setup();
    const other = { log: async () => [], overlay: async () => { throw new Error('unused'); }, close: vi.fn() };

    t.session.start(other, 'other', t.worktree);

    expect(t.isClosed()).toBe(true);
    expect(other.close).not.toHaveBeenCalled();
  });
});
