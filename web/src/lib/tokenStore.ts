// Access tokens of forge hosts (ADR 0022). A token is kept by origin, in the session storage
// by default and in the local storage only when the user asks; in memory when a storage
// refuses. It is never part of a URL, the hash, a log line, an error message or the cache.
import type { Fetch } from './forge';

/** Where a token is kept: this tab (session), this device (local), or memory only. */
export type Kept = 'session' | 'local' | 'memory';

export interface TokenStore {
  get(origin: string): string | undefined;
  /** `keep`: on this device (local storage); else for this tab only. */
  set(origin: string, token: string, keep: boolean): void;
  /** Removes the token of the origin from every place. */
  forget(origin: string): void;
  /** Where the token of the origin is, or null. */
  kept(origin: string): Kept | null;
}

const key = (origin: string) => `cg-token:${origin}`;

/**
 * The store over the two storages; each getter may throw or give undefined (private window,
 * blocked site data, no DOM), and then the token stays in memory for this page only.
 */
export function tokenStore(session: () => Storage | undefined, local: () => Storage | undefined): TokenStore {
  const memory = new Map<string, string>();
  const read = (s: () => Storage | undefined, origin: string): string | undefined => {
    try {
      return s()?.getItem(key(origin)) ?? undefined;
    } catch {
      return undefined;
    }
  };
  const write = (s: () => Storage | undefined, origin: string, token: string | null): boolean => {
    try {
      const st = s();
      if (!st) return false;
      if (token === null) st.removeItem(key(origin));
      else st.setItem(key(origin), token);
      return true;
    } catch {
      return false;
    }
  };
  const kept = (origin: string): Kept | null =>
    memory.has(origin) ? 'memory' : read(session, origin) !== undefined ? 'session' : read(local, origin) !== undefined ? 'local' : null;
  return {
    get: (origin) => memory.get(origin) ?? read(session, origin) ?? read(local, origin),
    set(origin, token, keep) {
      memory.delete(origin);
      write(keep ? session : local, origin, null);
      if (!write(keep ? local : session, origin, token)) memory.set(origin, token);
    },
    forget(origin) {
      memory.delete(origin);
      write(session, origin, null);
      write(local, origin, null);
    },
    kept,
  };
}

/** A request refused because its URL is not of the origin of the token (T-2). */
export class ForeignOrigin extends Error {}

/**
 * The network for one origin: each request must go to `origin`, else it is refused before
 * anything is sent, and carries the credential headers, if any; their form is the provider's
 * (Forgejo `Authorization: token`, GitHub `Bearer`, GitLab `PRIVATE-TOKEN`). A request with
 * credentials follows no redirect, so they cannot travel on to another host.
 */
export function authorized(fetch: Fetch, origin: string, credentials: Record<string, string> | undefined): Fetch {
  return async (url, init) => {
    let at: string;
    try {
      at = new URL(url).origin;
    } catch {
      at = 'null';
    }
    if (at !== origin) throw new ForeignOrigin(`refused a request to ${at} with the network of ${origin}`);
    const headers = { ...init?.headers };
    if (credentials === undefined) return fetch(url, { ...init, headers });
    return fetch(url, { ...init, headers: { ...headers, ...credentials }, redirect: 'error' });
  };
}

/** What the open field needs of the tokens; the app passes it to the field. */
export interface TokenField {
  /** The origin of the repository shown, or of the one the hash asks for, unless GitHub. */
  origin: string | null;
  /** Changes with each save and forget, so that the field reads `kept` again. */
  rev: number;
  kept: (origin: string) => Kept | null;
  save: (origin: string, token: string, keep: boolean) => void;
  /** Removes the token and the cached files (of all hosts: a text by blob sha does not know its host). */
  forget: (origin: string) => void;
  /** Opens the repository of the hash again, now with the token. */
  retry: () => void;
}
