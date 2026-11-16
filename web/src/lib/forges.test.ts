import { describe, expect, it } from 'vitest';
import { forgeFor, forgeOfRepo, kindOf, parseSource } from './forges';
import type { ForgeKind } from './forge';

const SHA = 'c38402e9e663c7e38b21f0082fd1fdcc4d97af59';
const ORIGIN = 'https://codeberg.org';
describe('parseSource', () => {
  const at = (owner: string, name: string, ref?: string, origin = ORIGIN) => ({ owner, name, origin, kind: 'forgejo', ...(ref === undefined ? {} : { ref }) });

  it('keeps owner/repo and github.com URLs for GitHub, without an origin', () => {
    expect(parseSource('a/b@v1')).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: 'v1' } });
    expect(parseSource('https://github.com/a/b/pull/4')).toMatchObject({ ok: true, repo: { owner: 'a', name: 'b' }, overlay: { kind: 'pull', n: 4 } });
  });

  it('reads the URLs of a Forgejo host: repository, branch, tag, commit tree, commit, compare, pull request', () => {
    expect(parseSource('https://codeberg.org/mergiraf/mergiraf')).toEqual({ ok: true, repo: at('mergiraf', 'mergiraf') });
    expect(parseSource('https://codeberg.org/mergiraf/mergiraf.git/')).toEqual({ ok: true, repo: at('mergiraf', 'mergiraf') });
    expect(parseSource('https://codeberg.org/o.x/r_1/src/branch/main')).toEqual({ ok: true, repo: at('o.x', 'r_1', 'main') });
    expect(parseSource('https://codeberg.org/o/r/src/tag/v1.0')).toEqual({ ok: true, repo: at('o', 'r', 'v1.0') });
    expect(parseSource(`https://codeberg.org/o/r/src/commit/${SHA}`)).toEqual({ ok: true, repo: at('o', 'r', SHA) });
    expect(parseSource(`https://codeberg.org/o/r/commit/${SHA}`)).toEqual({ ok: true, repo: at('o', 'r', SHA), overlay: { kind: 'commit' } });
    expect(parseSource('https://codeberg.org/o/r/compare/v1...main')).toEqual({ ok: true, repo: at('o', 'r', 'main'), overlay: { kind: 'range', base: 'v1', mergeBase: true } });
    expect(parseSource('https://codeberg.org/o/r/pulls/857/files')).toEqual({ ok: true, repo: at('o', 'r'), overlay: { kind: 'pull', n: 857 } });
  });

  it('takes the rest of a /src/branch/ URL as the branch name and says so', () => {
    expect(parseSource('https://codeberg.org/o/r/src/branch/feature/x')).toMatchObject({ ok: true, repo: at('o', 'r', 'feature/x'), note: expect.stringContaining('feature/x') });
  });

  it('reads kind:host/owner/repo@ref, the form of the hash, and host/owner/repo@ref, also with a range', () => {
    expect(parseSource(`forgejo:codeberg.org/o/r@${SHA}`)).toEqual({ ok: true, repo: at('o', 'r', SHA) });
    expect(parseSource(`forgejo:localhost:3000/o/r@${SHA}`)).toEqual({ ok: true, repo: at('o', 'r', SHA, 'http://localhost:3000') });
    expect(parseSource(`github:github.com/a/b@${SHA}`)).toEqual({ ok: true, repo: { owner: 'a', name: 'b', ref: SHA } });
    expect(parseSource(`codeberg.org/o/r@${SHA}`)).toEqual({ ok: true, repo: at('o', 'r', SHA) });
    expect(parseSource('git.example.org:3000/o/r@v1..v2')).toEqual({ ok: true, repo: at('o', 'r', 'v2', 'https://git.example.org:3000'), overlay: { kind: 'range', base: 'v1', mergeBase: false } });
  });

  it('allows http only for localhost', () => {
    expect(parseSource('http://localhost:3000/o/r')).toEqual({ ok: true, repo: at('o', 'r', undefined, 'http://localhost:3000') });
    expect(parseSource('localhost:3000/o/r@main')).toEqual({ ok: true, repo: at('o', 'r', 'main', 'http://localhost:3000') });
    expect(parseSource('http://codeberg.org/o/r')).toMatchObject({ ok: false, error: expect.stringContaining('https') });
  });

  it('rejects anything else with a message that shows the accepted forms', () => {
    for (const bad of ['https://codeberg.org/o', 'https://codeberg.org/o/r/issues/3', 'ftp://codeberg.org/o/r', 'https://codeberg.org/o/r/pulls/x', 'a/b/c/d'])
      expect(parseSource(bad), bad).toMatchObject({ ok: false, error: expect.stringContaining('/pulls/') });
  });
});

describe('kinds of forge', () => {
  it('knows github.com, codeberg.org and gitlab.com, and takes any other host as Forgejo for now', () => {
    expect(['github.com', 'codeberg.org', 'gitlab.com', 'git.example.org:3000'].map(kindOf)).toEqual(['github', 'forgejo', 'gitlab', 'forgejo']);
  });

  it('says that GitLab is not supported yet, for a URL, a host form and a hash', () => {
    for (const s of ['https://gitlab.com/group/sub/project', 'gitlab.com/g/p@main', `gitlab:git.example.org/g/sub/p@${SHA}`])
      expect(parseSource(s), s).toEqual({ ok: false, error: expect.stringContaining('GitLab is not supported yet') });
    expect(() => forgeFor('gitlab', 'https://gitlab.com', { fetch: async () => new Response() })).toThrow('GitLab is not supported yet');
  });

  it('refuses a kind it does not know, rather than give the links of another forge', () => {
    expect(() => forgeFor('gitea' as ForgeKind, 'https://git.example.org', { fetch: async () => new Response() })).toThrow('no forge of the kind "gitea"');
  });

  it('splits the path of the host form at its last /, so the kind decides how many segments an owner may have', () => {
    // Forgejo owners have one segment; the parse itself does not count them.
    expect(parseSource(`forgejo:codeberg.org/a/b/c@${SHA}`)).toMatchObject({ ok: false });
    expect(parseSource(`codeberg.org/a.b/c@${SHA}`)).toMatchObject({ ok: true, repo: { owner: 'a.b', name: 'c' } });
  });

  it('gives the forge of a kind: the host and the name come from it', () => {
    const net = { fetch: async () => new Response() };
    expect(forgeFor('github', 'https://github.com', net).host).toBe('github.com');
    expect(forgeFor('forgejo', 'https://codeberg.org', net, 't').name).toBe('codeberg.org');
  });

  it('gives the forge of a repository: GitHub without origin and kind, an error for one without the other', () => {
    const net = { fetch: async () => new Response() };
    expect(forgeOfRepo({ owner: 'a', name: 'b' }, net).host).toBe('github.com');
    expect(forgeOfRepo({ owner: 'a', name: 'b', origin: 'https://codeberg.org', kind: 'forgejo' }, net).host).toBe('codeberg.org');
    expect(() => forgeOfRepo({ owner: 'a', name: 'b', origin: 'https://codeberg.org' }, net)).toThrow('no kind of forge');
  });
});
