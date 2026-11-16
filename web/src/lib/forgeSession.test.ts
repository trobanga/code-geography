import { describe, expect, it } from 'vitest';
import { memoryCache, type Entry, type Forge, type Repo } from './forge';
import { forgeSession, NO_FORGE, shaOf, type ForgeState, type SessionDeps } from './forgeSession';
import { parseSource } from './forges';

const HEAD = 'a'.repeat(40);
const BASE = 'b'.repeat(40);
const isInput = (p: string) => p.endsWith('.java') || p === 'pom.xml';
const report = { calls: { to_function: 0, to_type: 0, external: 0, unresolved: 0 } };

/** Two commits; `main` is HEAD with parent BASE, BASE is a root commit. The blob sha is the text. */
const FILES: Record<string, Record<string, string>> = {
  [HEAD]: { 'pom.xml': '<p/>', 'A.java': 'class A { int x; }', 'New.java': 'class New {}' },
  [BASE]: { 'pom.xml': '<p/>', 'A.java': 'class A {}' },
};
const commit = (sha: string, parents: string[]) => ({ sha, parents, title: `t ${sha[0]}`, author: 'A', date: '2026-11-01T00:00:00Z' });

/** A fake forge, a memory cache and fake index and diff jobs; records what the session asks of the app. */
function setup() {
  const calls: string[] = [];
  const phases: (string | null)[] = [];
  const errors: string[] = [];
  const shown: { text: string; name: string; hash: string | undefined; at: string | null }[] = [];
  /** Answers of `resolve` held back until released: for stale replies. */
  const held = new Map<string, () => void>();
  const sha = (ref: string | undefined) => (ref === 'old' ? BASE : ref && /^[0-9a-f]{40}$/.test(ref) ? ref : HEAD);
  /** The origin of each repository that the session asked a forge for. */
  const origins: (string | undefined)[] = [];
  const forge: Forge = {
    host: 'example.org',
    name: 'Example',
    async resolve(repo: Repo) {
      calls.push(`resolve ${repo.ref}`);
      if (repo.ref?.startsWith('slow')) await new Promise<void>((ok) => held.set(repo.ref!, ok));
      return { sha: sha(repo.ref?.replace(/^slow-?/, '') || undefined) };
    },
    async tree(_repo, s) {
      calls.push(`tree ${s[0]}`);
      if (!FILES[s]) throw new Error(`o/r: HTTP 500`);
      return Object.entries(FILES[s]).map(([path, text]): Entry => ({ path, blobSha: `blob:${text}` }));
    },
    async text(_repo, s, path) {
      calls.push(`text ${s[0]} ${path}`);
      return FILES[s][path];
    },
    async commits(repo, s, n, page = 1) {
      calls.push(`commits ${s[0]} ${n}${page > 1 ? ` page ${page}` : ''}`);
      if (repo.name === 'broken') throw new Error('o/broken: HTTP 500');
      if (s.startsWith('dead')) throw new Error(`o/r@${s.slice(0, 7)}: not found`);
      // A repository with a history of 70 commits.
      if (repo.name === 'long') return Array.from({ length: 70 }, (_, i) => commit(`c${i}`, [`c${i + 1}`])).slice((page - 1) * n, page * n);
      // A repository whose newest `n` commits are all of an update bot.
      if (repo.name === 'updates' && page === 1) return Array.from({ length: n }, (_, i) => ({ ...commit(`bot${i}`, []), author: 'renovate[bot]' }));
      if (page > 1 && repo.name !== 'updates') return [];
      return [commit(HEAD, [BASE]), commit(BASE, [])].filter((c) => c.sha === sha(s) || c.sha === BASE).slice(0, n);
    },
    async mergeBase() {
      throw new Error('unused');
    },
    async pull() {
      throw new Error('unused');
    },
    async branches(repo) {
      calls.push(`branches ${repo.name}`);
      if (repo.name === 'broken') throw new Error('o/broken: HTTP 500');
      return [
        { name: 'main', sha: HEAD },
        { name: 'old', sha: BASE },
      ];
    },
  };
  let seq = 0;
  let state: ForgeState = NO_FORGE;
  const cache = memoryCache();
  const deps: SessionDeps = {
    forge: (repo) => {
      origins.push(repo.origin);
      return forge;
    },
    parse: parseSource,
    cache: async () => cache,
    isInput,
    ready: async () => {},
    index: async (req) => {
      calls.push('index');
      return { graphText: JSON.stringify({ nodes: req.paths.map((p) => ({ id: p, parent: null, kind: 'type', name: p })), edges: [] }), report };
    },
    diff: async (req) => {
      calls.push(`diff ${req.baseName}..${req.headName}`);
      const nodes = [
        { id: 'A', parent: null, kind: 'type', name: 'A', change: 'modified' },
        { id: 'Gone', parent: null, kind: 'type', name: 'Gone', change: 'removed' },
      ];
      return { graphText: JSON.stringify({ nodes, edges: [], diff: { base: req.baseName, head: req.headName } }), report };
    },
    begin: () => {
      const my = ++seq;
      return () => my === seq;
    },
    phase: (p) => phases.push(p),
    fail: (m) => errors.push(m),
    show: async (text, summary, hash) => void shown.push({ text, name: summary.name, hash, at: state.at }),
    hash: () => '#now',
    onState: (s) => (state = s),
  };
  return { session: forgeSession(deps), origins, calls, phases, errors, shown, held, state: () => state, cancelOther: () => ++seq };
}

const REPO = { owner: 'o', name: 'r' };

/** Lets the pending promises run until `done`. */
async function until(done: () => boolean) {
  for (let i = 0; i < 200 && !done(); i++) await Promise.resolve();
}

/** Lets the pending promises run until `resolve` of `ref` waits, then releases it. */
async function release(held: Map<string, () => void>, ref: string) {
  await until(() => held.has(ref));
  held.get(ref)!();
}

describe('forgeSession', () => {
  it('opens a repository of another host with the forge of its origin, at `kind:host/owner/repo@sha`, and keeps the origin for the commit list', async () => {
    const t = setup();
    expect(t.session.openInput('https://git.example.org/o/r/src/branch/main')).toBeNull();
    await until(() => t.shown.length > 0);
    expect(t.state()).toMatchObject({ at: `forgejo:git.example.org/o/r@${HEAD}`, repo: { ...REPO, origin: 'https://git.example.org', kind: 'forgejo' } });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    expect(new Set(t.origins)).toEqual(new Set(['https://git.example.org']));
    // The hash form of `at` opens the same commit and costs no open.
    expect(t.session.followHash(`forgejo:git.example.org/o/r@${HEAD}`, '#h', undefined)).toBe(false);
  });

  it('opens a repository: shows the head graph, at `owner/repo@sha`', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' }, undefined, '#view');
    expect(t.shown).toHaveLength(1);
    expect(JSON.parse(t.shown[0].text).nodes.map((n: { id: string }) => n.id)).toEqual(['A.java', 'New.java', 'pom.xml']);
    expect(t.shown[0]).toMatchObject({ name: `o/r@main (${HEAD.slice(0, 7)})`, hash: '#view', at: `o/r@${HEAD}` });
    expect(t.state()).toMatchObject({ at: `o/r@${HEAD}`, mode: 'graph', overlay: null, repo: REPO });
  });

  it('reports the progress: resolve, list, n / m files, index, done', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    expect(t.phases).toEqual([
      'opening o/r',
      'resolving o/r@main',
      `listing the files of o/r@${HEAD.slice(0, 7)}`,
      'fetched 1 / 3 files of o/r',
      'fetched 2 / 3 files of o/r',
      'fetched 3 / 3 files of o/r',
      'indexing 3 files of o/r',
      null,
    ]);
  });

  it('a failed open reports the message of the forge and leaves the repository shown before', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    const before = t.state();
    await t.session.open({ ...REPO, ref: 'c'.repeat(40) });
    expect(t.errors).toEqual(['cannot open o/r: o/r: HTTP 500']);
    expect(t.phases.at(-1)).toBeNull();
    expect(t.shown).toHaveLength(1);
    expect(t.state()).toBe(before);
  });

  it('a stale reply changes nothing: the later open wins', async () => {
    const t = setup();
    const first = t.session.open({ ...REPO, ref: 'slow-old' });
    await until(() => t.held.has('slow-old'));
    await t.session.open({ ...REPO, ref: 'main' });
    const phases = t.phases.length;
    await release(t.held, 'slow-old');
    await first;
    expect(t.phases).toHaveLength(phases);
    expect(t.shown.map((s) => s.at)).toEqual([`o/r@${HEAD}`]);
    expect(t.state().at).toBe(`o/r@${HEAD}`);
  });

  it('builds the overlay of a range: shows it, keeps the head without the removed nodes', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD }, '#view', 'a note');
    expect(t.calls).toContain('diff bbbbbbb..aaaaaaa');
    expect(t.calls.filter((c) => c.startsWith('resolve'))).toEqual([]);
    expect(t.shown).toHaveLength(1);
    expect(t.shown[0]).toMatchObject({ name: 'o/r bbbbbbb → aaaaaaa', hash: '#view', at: `o/r@${HEAD}` });
    const s = t.state();
    expect(s).toMatchObject({ at: `o/r@${HEAD}`, mode: 'commit', repo: REPO });
    expect(s.overlay!.range).toEqual({ base: BASE, head: HEAD });
    expect(s.overlay!.summary.text).toMatch(/, 2 changed files \(a note\)$/);
    expect(JSON.parse(s.head!.text)).toEqual({ nodes: [{ id: 'A', parent: null, kind: 'type', name: 'A' }], edges: [] });
    expect(s.head!.summary.name).toBe('o/r@aaaaaaa');
  });

  it('switches to the head and back to the overlay without a fetch; the view stays', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    const fetched = t.calls.length;
    await t.session.show('graph');
    expect(t.state().mode).toBe('graph');
    await t.session.show('graph');
    await t.session.show('commit');
    expect(t.state().mode).toBe('commit');
    expect(t.calls).toHaveLength(fetched);
    expect(t.shown.map((s) => [s.name, s.hash])).toEqual([
      ['o/r bbbbbbb → aaaaaaa', undefined],
      ['o/r@aaaaaaa', '#now'],
      ['o/r bbbbbbb → aaaaaaa', '#now'],
    ]);
  });

  it('the overlay that is kept, asked for again, shows it from memory with the view shown now', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    await t.session.show('graph');
    const fetched = t.calls.length;
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    expect(t.calls).toHaveLength(fetched);
    expect(t.shown.at(-1)).toMatchObject({ name: 'o/r bbbbbbb → aaaaaaa', hash: '#now' });
    expect(t.state().mode).toBe('commit');
  });

  it('restores `gh=owner/repo@sha` from the hash with its view', async () => {
    const t = setup();
    t.session.followHash(`o/r@${HEAD}`, '#gh=x&open=A', undefined);
    await until(() => t.shown.length > 0);
    expect(t.shown[0]).toMatchObject({ at: `o/r@${HEAD}`, hash: '#gh=x&open=A', name: 'o/r@aaaaaaa' });
    expect(t.state().mode).toBe('graph');
  });

  it('restores `gh=…&base=…` of two full shas as the same overlay, without a resolve', async () => {
    const t = setup();
    t.session.followHash(`o/r@${HEAD.toUpperCase()}`, '#v', BASE.toUpperCase());
    await until(() => t.shown.length > 0);
    expect(t.calls.filter((c) => c.startsWith('resolve') || c.startsWith('commits'))).toEqual([]);
    expect(t.state().overlay!.range).toEqual({ base: BASE, head: HEAD });
    expect(t.shown[0]).toMatchObject({ hash: '#v', at: `o/r@${HEAD}` });
  });

  it('resolves a hand-written ref of the hash: `base=<ref>` as a range, `base=none` as the commit against its first parent', async () => {
    const t = setup();
    t.session.followHash('o/r@main', '#v', 'old');
    await until(() => t.shown.length > 0);
    expect(t.calls.filter((c) => c.startsWith('resolve'))).toEqual(['resolve main', 'resolve old']);
    expect(t.state().overlay!.range).toEqual({ base: BASE, head: HEAD });
    t.session.followHash('o/r@main', '#v', null);
    await until(() => t.calls.some((c) => c.startsWith('commits')));
    expect(t.calls.filter((c) => c.startsWith('commits'))).toEqual(['commits m 1']);
  });

  it('restores `base=none` of a full sha as the overlay of a root commit', async () => {
    const t = setup();
    t.session.followHash(`o/r@${BASE}`, '#v', null);
    await until(() => t.shown.length > 0);
    expect(t.state().overlay!.range).toEqual({ base: null, head: BASE });
    expect(t.calls).toContain('diff none..bbbbbbb');
  });

  it('a hash that names what is shown opens nothing; the same commit without `base` is the head from memory', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    const fetched = t.calls.length;
    expect(t.session.followHash(`o/r@${HEAD}`, '#v', BASE)).toBe(false);
    expect(t.session.followHash(`o/r@${HEAD}`, '#w', undefined)).toBe(true);
    await until(() => t.shown.length > 1);
    expect(t.calls).toHaveLength(fetched);
    expect(t.shown[1]).toMatchObject({ name: 'o/r@aaaaaaa', hash: '#w' });
    expect(t.session.followHash(`o/r@${HEAD}`, '#v', undefined)).toBe(false);
  });

  it('the commit list: one call on the first open of the list, anchored at the sha shown; kept while it closes and opens', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    expect(t.state()).toMatchObject({ listOpen: true, listStatus: 'listing the last 30 commits' });
    await until(() => t.state().commits !== null);
    expect(t.state()).toMatchObject({ listStatus: null, commits: { anchor: HEAD, list: [{ sha: HEAD }, { sha: BASE }] } });
    t.session.toggleList();
    t.session.toggleList();
    await until(() => false);
    expect(t.calls.filter((c) => c.startsWith('commits'))).toEqual(['commits a 30']);
    expect(t.state().listOpen).toBe(true);
  });

  it('the commit list asks for a further page if update bots made the commits of the first one', async () => {
    const t = setup();
    await t.session.open({ owner: 'o', name: 'updates', ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    expect(t.calls.filter((c) => c.startsWith('commits'))).toEqual(['commits a 30', 'commits a 30 page 2']);
    expect(t.state().commits?.list.slice(-2)).toMatchObject([{ sha: HEAD }, { sha: BASE }]);
  });

  it('loadMore appends the next 30 commits to the list, until the history ends; then it asks no more', async () => {
    const t = setup();
    await t.session.open({ owner: 'o', name: 'long', ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    expect(t.state().commits).toMatchObject({ next: 2 });
    const more = t.session.loadMore();
    expect(t.state().listStatus).toBe('listing 30 more commits');
    // The rows stay while the next ones load.
    expect(t.state().commits!.list).toHaveLength(30);
    await more;
    expect(t.state()).toMatchObject({ listStatus: null, commits: { anchor: HEAD, next: 3 } });
    expect(t.state().commits!.list.map((c) => c.sha)).toEqual(Array.from({ length: 60 }, (_, i) => `c${i}`));
    await t.session.loadMore();
    expect(t.state().commits).toMatchObject({ next: null });
    expect(t.state().commits!.list).toHaveLength(70);
    await t.session.loadMore();
    expect(t.calls.filter((c) => c.startsWith('commits'))).toEqual(['commits a 30', 'commits a 30 page 2', 'commits a 30 page 3']);
  });

  it('the switch to the commit view without an overlay opens the commit list', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.switchTo('commit');
    expect(t.state()).toMatchObject({ mode: 'commit', listOpen: true });
    await until(() => t.state().commits !== null);
    expect(t.shown).toHaveLength(1);
  });

  it('a commit picked from the list: its overlay against the first parent; the list closes at once and keeps its commits; an unknown sha does nothing', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.switchTo('commit');
    await until(() => t.state().commits !== null);
    t.session.pick('nope');
    expect(t.state().listOpen).toBe(true);
    const picked = t.session.pick(HEAD);
    expect(t.state().listOpen).toBe(false);
    await picked;
    // The listed commit goes along, so that its message costs no further call.
    expect(t.state().overlay!.range).toMatchObject({ base: BASE, head: HEAD, commit: { sha: HEAD } });
    expect(t.state()).toMatchObject({ listOpen: false, commits: { anchor: HEAD } });
    expect(t.shown).toHaveLength(2);
  });

  it('a pasted sha of a listed commit, full or short: the overlay of that commit with no call for it; the list keeps its commits', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    const picked = t.session.pickSha('bbbbbbb');
    expect(t.state().listOpen).toBe(false);
    await picked;
    expect(t.state().overlay!.range).toMatchObject({ base: null, head: BASE, commit: { sha: BASE } });
    expect(t.state()).toMatchObject({ commits: { anchor: HEAD } });
    expect(t.calls.filter((c) => c.startsWith('commits'))).toEqual(['commits a 30']);
  });

  it('a pasted sha that the list does not hold: one call resolves it, then its overlay; the list and the branch go, since the commit can be on another branch', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    await t.session.pickBranch('old');
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    expect(t.state()).toMatchObject({ branch: 'old', commits: { anchor: BASE, list: [{ sha: BASE }] } });
    const picked = t.session.pickSha('aaaaaaa');
    expect(t.state().listOpen).toBe(false);
    await picked;
    expect(t.calls.filter((c) => c.startsWith('commits'))).toEqual(['commits b 30', 'commits a 1']);
    expect(t.state().overlay!.range).toMatchObject({ base: BASE, head: HEAD, commit: { sha: HEAD } });
    expect(t.state()).toMatchObject({ at: `o/r@${HEAD}`, commits: null, branch: null });
  });

  it('a pasted sha that the forge does not know: its message; the picture, the list and the branch stay', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    await t.session.pickSha('dead'.repeat(10));
    expect(t.errors).toEqual(['cannot build the overlay of o/r: o/r@deaddea: not found']);
    expect(t.state()).toMatchObject({ at: `o/r@${HEAD}`, mode: 'graph', overlay: null, commits: { anchor: HEAD } });
    expect(t.shown).toHaveLength(1);
  });

  it('closeList closes the open list and keeps its commits; on a closed list it does nothing', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    t.session.closeList();
    expect(t.state()).toMatchObject({ listOpen: false, commits: { anchor: HEAD } });
    const before = t.state();
    t.session.closeList();
    expect(t.state()).toBe(before);
  });

  it('the input field: an error for what is not a repository; an overlay for a range; a note in the summary', async () => {
    const t = setup();
    expect(t.session.openInput('nonsense')).toMatch(/^not a repository: "nonsense"/);
    expect(t.calls).toEqual([]);
    expect(t.session.openInput(`o/r@${BASE}..${HEAD}`)).toBeNull();
    await until(() => t.shown.length > 0);
    expect(t.state().overlay!.range).toEqual({ base: BASE, head: HEAD });
    t.session.openInput('https://github.com/o/r/tree/feature/x');
    await until(() => t.shown.length > 1);
    expect(t.calls).toContain('resolve feature/x');
    expect(t.state().head!.summary.text).toMatch(/\(took "feature\/x" as the whole branch name; a path inside a branch is not supported\)$/);
  });

  it('forget: another source is shown; nothing of the repository stays', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.forget();
    expect(t.state()).toEqual(NO_FORGE);
    await t.session.show('commit');
    expect(t.state()).toEqual(NO_FORGE);
  });

  it('a new open keeps the commit list only for the same sha, and closes it', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    await t.session.open({ ...REPO, ref: HEAD });
    expect(t.state()).toMatchObject({ listOpen: false, commits: { anchor: HEAD } });
    await t.session.open({ ...REPO, ref: 'old' });
    expect(t.state().commits).toBeNull();
  });

  it('a failed commit list gives its message in the list', async () => {
    const t = setup();
    await t.session.open({ owner: 'o', name: 'broken', ref: 'main' });
    t.session.toggleList();
    await until(() => t.state().listStatus !== 'listing the last 30 commits');
    expect(t.state()).toMatchObject({ commits: null, listStatus: 'cannot list the commits: o/broken: HTTP 500' });
  });

  it('a commit list for a repository no longer shown is dropped', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    t.session.forget();
    await until(() => false);
    expect(t.state()).toEqual(NO_FORGE);
  });

  it('another open of the app (a directory) makes a pending open stale, also its failure', async () => {
    const t = setup();
    const first = t.session.open({ ...REPO, ref: 'slow-' + 'c'.repeat(40) });
    t.cancelOther();
    await release(t.held, 'slow-' + 'c'.repeat(40));
    await first;
    expect(t.errors).toEqual([]);
    expect(t.shown).toEqual([]);
  });
});

describe('the branch menu', () => {
  it('toggleBranches opens the menu and lists the branches of the repository shown', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    expect(t.state().branchOpen).toBe(true);
    await until(() => t.state().branches !== null);
    expect(t.state().branches).toEqual([
      { name: 'main', sha: HEAD },
      { name: 'old', sha: BASE },
    ]);
    expect(t.calls.filter((c) => c.startsWith('branches'))).toEqual(['branches r']);
  });

  it('shows a status while it waits, and the message of the forge when the listing fails', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    expect(t.state().branchStatus).toBe('listing the branches');
    await until(() => t.state().branches !== null);
    expect(t.state().branchStatus).toBeNull();
    const u = setup();
    await u.session.open({ owner: 'o', name: 'broken', ref: 'main' });
    u.session.toggleBranches();
    await until(() => u.state().branchStatus !== 'listing the branches');
    expect(u.state()).toMatchObject({ branchOpen: true, branches: null, branchStatus: 'cannot list the branches: o/broken: HTTP 500' });
  });

  it('a second open of the menu lists the branches again and shows the list of before meanwhile; a close fetches nothing', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    t.session.toggleBranches();
    expect(t.state().branchOpen).toBe(false);
    t.session.toggleBranches();
    expect(t.state()).toMatchObject({ branchOpen: true, branchStatus: 'listing the branches', branches: [{ name: 'main' }, { name: 'old' }] });
    await until(() => t.state().branchStatus === null);
    expect(t.calls.filter((c) => c.startsWith('branches'))).toEqual(['branches r', 'branches r']);
  });

  it('pickBranch closes the menu at once and opens the head of that branch as the plain graph with a fresh view', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    const picked = t.session.pickBranch('old');
    expect(t.state().branchOpen).toBe(false);
    await picked;
    expect(t.calls).toContain('resolve old');
    expect(t.shown.at(-1)).toMatchObject({ name: `o/r@old (${BASE.slice(0, 7)})`, hash: undefined, at: `o/r@${BASE}` });
    expect(t.state()).toMatchObject({ mode: 'graph', overlay: null, branch: 'old', repo: REPO });
  });

  it('pickBranch of a name that the menu does not list does nothing', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    const before = t.state();
    const fetched = t.calls.length;
    await t.session.pickBranch('nope');
    expect(t.state()).toBe(before);
    expect(t.calls).toHaveLength(fetched);
  });

  it('a pick from the commit list keeps the branch; any other open or overlay resets it', async () => {
    const t = setup();
    const onBranch = async () => {
      t.session.toggleBranches();
      await until(() => t.state().branchStatus === null);
      await t.session.pickBranch('main');
      expect(t.state().branch).toBe('main');
    };
    await t.session.open({ ...REPO, ref: 'main' });
    await onBranch();
    t.session.toggleList();
    await until(() => t.state().commits !== null);
    await t.session.pick(HEAD);
    expect(t.state()).toMatchObject({ mode: 'commit', branch: 'main' });
    await t.session.open({ ...REPO, ref: 'main' });
    expect(t.state().branch).toBeNull();
    await onBranch();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    expect(t.state().branch).toBeNull();
  });

  it('the branch menu and the commit list close each other when one opens', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleList();
    t.session.toggleBranches();
    expect(t.state()).toMatchObject({ branchOpen: true, listOpen: false });
    t.session.toggleList();
    expect(t.state()).toMatchObject({ branchOpen: false, listOpen: true });
  });

  it('closeBranches closes the open menu and keeps its branches; on a closed menu it does nothing', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    t.session.closeBranches();
    expect(t.state()).toMatchObject({ branchOpen: false, branches: [{ name: 'main' }, { name: 'old' }] });
    const before = t.state();
    t.session.closeBranches();
    expect(t.state()).toBe(before);
  });

  it('a reply for a repository no longer shown changes nothing, also a failed one', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    t.session.forget();
    await until(() => false);
    expect(t.state()).toEqual(NO_FORGE);
    await t.session.open({ owner: 'o', name: 'broken', ref: 'main' });
    t.session.toggleBranches();
    t.session.forget();
    await until(() => false);
    expect(t.state()).toEqual(NO_FORGE);
  });

  it('keeps the branches for another commit of the same repository and drops them for another repository', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    await t.session.pickBranch('old');
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    expect(t.state().branches).toHaveLength(2);
    await t.session.open({ owner: 'o', name: 'other', ref: 'main' });
    expect(t.state().branches).toBeNull();
    t.session.toggleBranches();
    await until(() => t.state().branches !== null);
    await t.session.overlay({ owner: 'o', name: 'r', ref: HEAD }, { base: BASE, head: HEAD });
    expect(t.state().branches).toBeNull();
  });
});

describe('a choice of the user while an open is pending (F93)', () => {
  it('a switch to the head makes the pending open stale and ends its progress line', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    const pending = t.session.open({ ...REPO, ref: 'slow-old' });
    await until(() => t.held.has('slow-old'));
    await t.session.show('graph');
    await release(t.held, 'slow-old');
    await pending;
    expect(t.state()).toMatchObject({ at: `o/r@${HEAD}`, mode: 'graph' });
    expect(t.shown.map((s) => s.name)).toEqual(['o/r bbbbbbb → aaaaaaa', 'o/r@aaaaaaa']);
    expect(t.phases.at(-1)).toBeNull();
  });

  it('a switch to the overlay makes the pending open stale', async () => {
    const t = setup();
    await t.session.overlay({ ...REPO, ref: HEAD }, { base: BASE, head: HEAD });
    await t.session.show('graph');
    const pending = t.session.open({ ...REPO, ref: 'slow-old' });
    await until(() => t.held.has('slow-old'));
    await t.session.show('commit');
    await release(t.held, 'slow-old');
    await pending;
    expect(t.state()).toMatchObject({ at: `o/r@${HEAD}`, mode: 'commit' });
    expect(t.phases.at(-1)).toBeNull();
  });

  it('another source (a graph file) makes the pending open stale', async () => {
    const t = setup();
    await t.session.open({ ...REPO, ref: 'main' });
    const pending = t.session.open({ ...REPO, ref: 'slow-old' });
    await until(() => t.held.has('slow-old'));
    t.session.forget();
    await release(t.held, 'slow-old');
    await pending;
    expect(t.state()).toEqual(NO_FORGE);
    expect(t.shown).toHaveLength(1);
    expect(t.phases.at(-1)).toBeNull();
  });
});

describe('shaOf', () => {
  it('is the commit sha of a repository shown, on GitHub and on another forge', () => {
    const sha = '0123456789abcdef0123456789abcdef01234567';
    expect(shaOf(`o/r@${sha}`)).toBe(sha);
    expect(shaOf(`forgejo:codeberg.org/o/r@${sha}`)).toBe(sha);
  });
});
