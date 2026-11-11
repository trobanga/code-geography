import { describe, expect, it } from 'vitest';
import type { Fetch } from './forge';
import { authorized, tokenStore } from './tokenStore';

/** A `Storage` in memory, enough for the store. */
function fakeStorage(): Storage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    get length() {
      return data.size;
    },
    key: (i) => [...data.keys()][i] ?? null,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, String(v)),
    removeItem: (k) => void data.delete(k),
    clear: () => data.clear(),
  };
}

const HOST = 'https://forge.example.org';

describe('tokenStore', () => {
  it('keeps a token in the session storage by default, by host', () => {
    const session = fakeStorage();
    const local = fakeStorage();
    const s = tokenStore(() => session, () => local);
    s.set(HOST, 'secret', false);
    expect(s.get(HOST)).toBe('secret');
    expect(s.get('https://other.example.org')).toBeUndefined();
    expect(local.data.size).toBe(0);
    expect(s.kept(HOST)).toBe('session');
  });

  it('keeps it in the local storage only when asked, and then not also in the session storage', () => {
    const session = fakeStorage();
    const local = fakeStorage();
    const s = tokenStore(() => session, () => local);
    s.set(HOST, 'one', false);
    s.set(HOST, 'two', true);
    expect(s.get(HOST)).toBe('two');
    expect([...session.data.values()]).toEqual([]);
    expect(s.kept(HOST)).toBe('local');
    // And back: a token for this tab only removes the one on the device.
    s.set(HOST, 'three', false);
    expect([...local.data.values()]).toEqual([]);
  });

  it('forgets the token of a host in both storages and leaves other hosts', () => {
    const session = fakeStorage();
    const local = fakeStorage();
    const s = tokenStore(() => session, () => local);
    session.setItem(`cg-token:${HOST}`, 'a');
    local.setItem(`cg-token:${HOST}`, 'b');
    s.set('https://other.example.org', 'c', true);
    s.forget(HOST);
    expect(s.get(HOST)).toBeUndefined();
    expect(s.kept(HOST)).toBeNull();
    expect(s.get('https://other.example.org')).toBe('c');
  });

  it('keeps the token in memory when the storages throw, as in a private window', () => {
    const refuse = () => {
      throw new DOMException('denied', 'SecurityError');
    };
    const s = tokenStore(refuse, refuse);
    s.set(HOST, 'secret', true);
    expect(s.get(HOST)).toBe('secret');
    expect(s.kept(HOST)).toBe('memory');
    s.forget(HOST);
    expect(s.get(HOST)).toBeUndefined();
  });
});

describe('authorized', () => {
  function recording() {
    const seen: { url: string; init: Parameters<Fetch>[1] }[] = [];
    const fetch: Fetch = async (url, init) => {
      seen.push({ url, init });
      return new Response('ok');
    };
    return { fetch, seen };
  }

  it('adds the credential headers to a request to its own origin, and follows no redirect with it', async () => {
    const { fetch, seen } = recording();
    await authorized(fetch, HOST, { Authorization: 'token secret' })(`${HOST}/api/v1/repos/o/r`, { headers: { Accept: 'x' } });
    expect(seen[0].init?.headers).toEqual({ Accept: 'x', Authorization: 'token secret' });
    expect(seen[0].init?.redirect).toBe('error');
  });

  it('refuses a URL of any other origin, also one that only looks alike, without sending anything', async () => {
    const { fetch, seen } = recording();
    const send = authorized(fetch, HOST, { Authorization: 'token secret' });
    for (const url of [
      'https://evil.example.org/api/v1/x',
      'http://forge.example.org/api/v1/x',
      'https://forge.example.org:8443/api/v1/x',
      'https://forge.example.org.evil.net/x',
      'https://forge.example.org@evil.net/x',
      '//evil.net/x',
      'not a url',
    ]) {
      const e = await send(url).then(() => null, (x: unknown) => x as Error);
      expect(e, url).toBeInstanceOf(Error);
      expect(e!.message, url).not.toContain('secret');
    }
    expect(seen).toEqual([]);
  });

  it('sends no credential header without one', async () => {
    const { fetch, seen } = recording();
    await authorized(fetch, HOST, undefined)(`${HOST}/api/v1/x`);
    expect(seen[0].init?.headers).toEqual({});
  });
});
