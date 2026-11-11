# 22. A Forgejo host opens in the browser, a private repository with a token

Status: accepted (2026-11-11)

## Context
Private repositories will live on a self-hosted Forgejo. The viewer runs on another host than
Forgejo, so each request is cross-origin. No OAuth for now: the user gives an access token.
No private instance was available; the facts below were measured on Codeberg (Forgejo
17.0, Gitea API 1.22) on 2026-11-11, and the token path ran against a mock only.
- `/api/v1` answers with `access-control-allow-origin: *`, also on 401 and 404, and a
  preflight with `Access-Control-Request-Headers: authorization` gets
  `access-control-allow-headers: Authorization`. The web route `/{o}/{r}/raw/commit/...`
  has no CORS header, and its preflight gets 405: only the API can serve a browser.
- `GET /repos/{o}/{r}/git/commits/{ref}` takes a branch, a tag, a short sha or `HEAD`; an
  unknown ref gives 404 (`/commits/{ref}` as on GitHub does not exist: 404).
- `GET /repos/{o}/{r}/git/trees/{sha}?recursive=true&page=n` is paged, 1000 entries per
  page (also with `per_page=10000`), and counts trees and blobs in `total_count`.
  `truncated` is still `true` on the last page with entries and turns `false` only on the
  empty page after it.
- `GET /repos/{o}/{r}/raw/{path}?ref={sha}` gives the file, `text/plain; charset=utf-8`.
- `GET /repos/{o}/{r}/commits?sha=&limit=` gives sha, parents and the message.
- `GET /repos/{o}/{r}/compare/{a}...{b}` lists the commits from the merge base to `b`,
  without the merge base, and has no field for it. A pull request has `merge_base`.
- 401 for a wrong token; its body repeats the token (`access token does not exist [sha: <token>]`).
  404 for a repository that does not exist. Forgejo answers 404 also for a private
  repository without access (from the source, not measured). Rate limit on Codeberg:
  `ratelimit-policy: "baseline";q=2000;w=600`, a header the browser cannot read (not exposed).
- Forgejo's source (`modules/setting/cors.go`, `routers/api/shared/middleware.go`):
  `[cors] ENABLED` is `false` by default, and then no CORS header is sent. When enabled,
  the API allows the header `Authorization` in any case; `HEADERS` only adds to it.

## Decision
- `forgejo(net, origin, token?)` in `web/src/lib/forgejo.ts` is the second `Forge` (ADR 20).
  `resolve` 1 call (none for a full sha), `tree` 1 call per 1000 entries, `text` 1 call per
  file from the raw API, `commits` 1 call, `pull` 1 call (it carries the merge base).
  `mergeBase` reads the compare: each parent of a listed commit that is not listed is the
  merge base or an ancestor of it, and the merge base is one of them; of several, one more
  compare each picks it (`x...y` lists nothing when y is an ancestor of x). A compare that
  lists fewer commits than `total_commits` is refused.
- A tree is read until it has `total_count` entries; an empty page before that, or more
  entries, is refused, as is a truncated tree on GitHub.
- The kind of a forge is data: `ForgeKind = github | forgejo | gitlab`. `web/src/lib/forges.ts`
  is the one place that maps a host to a kind (`kindOf`), a kind to a `Forge` (`forgeFor`)
  and the input or the hash to a repository (`parseSource`). Known hosts: `github.com` →
  github, `codeberg.org` → forgejo, `gitlab.com` → gitlab. Any other host is forgejo for
  now, through `kindOf` only, so that a probe of the host or a choice of the user can replace
  it there. Kind `gitlab` gives "GitLab is not supported yet" in the parse and in `forgeFor`.
- Accepted: `owner/repo[@ref]` and `github.com` URLs (GitHub); for Forgejo `https://<host>/o/r`
  with `/src/branch|tag|commit/<ref>`, `/commit/<sha>`, `/compare/A...B`, `/pulls/<n>`;
  `host/path[@ref|@A..B|@A...B]`; and the hash form `kind:host/path@ref`. `path` is split
  at its last `/` into owner and name; the kind says how many segments an owner may have
  (Forgejo: one). Only `https:`; `http:` only for `localhost`, `127.0.0.1` and `[::1]`,
  which the host form also gets by default. Forgejo under a sub-path is not supported.
- The repository carries its origin and kind (`Repo.origin`, `Repo.kind`, both absent for
  GitHub). The session asks for a forge per open (`forge(repo)`), built with the token
  stored for that origin at that moment, so a token given later applies to the next open
  and no forge outlives a switch.
- Hash: `forge=<kind>:<host>/<path>@<sha>` (e.g. `forge=forgejo:codeberg.org/o/r@<sha>`), with
  `base=` as for `gh=`, which stays the short form for GitHub. A shared link names its kind.
- Tokens:
  - T-1. A token is never in the URL, the hash, a log line, an error message or the cache.
    Error messages never contain the body of an answer (a 401 body repeats the token).
  - T-2. `authorized(fetch, origin, credentials)` in `tokenStore.ts` is the only place that
    adds credential headers. Their form is the provider's (Forgejo `Authorization: token <t>`;
    later GitHub `Bearer`, GitLab `PRIVATE-TOKEN`); the store is keyed by origin and knows
    no kind. It refuses any URL whose origin differs from its own before
    anything is sent, and a request with a token follows no redirect (`redirect: 'error'`).
    `forgejo.ts` builds every URL from the origin and a path and never follows a URL of an
    answer (`url`, `Link`).
  - T-3. `tokenStore(session, local)`: by origin; `sessionStorage` by default, `localStorage`
    only with "keep on this device", never both; in memory when a storage throws. "forget
    token" removes it from both and clears the text cache.
  - T-4. The field is `type="password"`, `autocomplete="off"`, emptied after use; the page
    then shows only where the token is kept.
  - T-5. The help asks for a token with the scope `read:repository` only.
- What the Forgejo admin sets in `app.ini` so that a viewer at `https://viewer.example.org`
  can read with a token:

  ```ini
  [cors]
  ENABLED = true
  ALLOW_DOMAIN = https://viewer.example.org
  ; optional, the viewer sends only GET (and the preflight OPTIONS):
  METHODS = GET,OPTIONS
  ```

  `ALLOW_CREDENTIALS` stays `false`: the viewer sends no cookies. `HEADERS` needs no change.
  Without `ENABLED`, `fetch` fails with a `TypeError` whose reason the browser hides; the
  viewer then names CORS and these lines.

## Consequences
- Codeberg, explosion-mental/wallust (38 inputs), Firefox 159 through a proxy: first open
  40 API calls in 24 s, the overlay of one commit 9 calls in 12 s, each reload 0 calls in
  0.4 s. Codeberg answered a single file in 1.5 to 5 s; a 600-file repository did not open
  within 90 s. The API is the only source with CORS, so each file is one API call.
- With a token each request is preflighted, and the preflight cache is per URL, so each
  file costs two requests.
- The text cache stays keyed by blob sha, shared by all hosts. A text can only come back for
  a tree that lists the same blob sha, that is the same content, so no other repository
  sees a text it does not hold itself. But private texts stay in IndexedDB of this browser
  until "forget token", which clears the cache of all hosts, since a text does not know its
  host.
- A renamed repository answers with a redirect; with a token that fails as a network error.
- No CSP: with the host free input, `connect-src` cannot be a fixed list. A CSP could only
  name the hosts of one deployment (`connect-src 'self' https://api.github.com
  https://raw.githubusercontent.com https://forge.example.org`), which would also forbid any
  other host. Left to the deployment.
- Open points for GitLab (planned, not built): API v4 (`/api/v4/projects/:id/...`) with the
  project id or the URL-encoded full path (`group%2Fsub%2Fproject`) instead of owner and name;
  nested groups, so `Repo` with exactly one owner segment does not fit; header
  `PRIVATE-TOKEN` or `Authorization: Bearer`; trees paged by `page`/`per_page` with `X-Next-Page`;
  raw files at `/repository/files/:path/raw?ref=`; merge base at `/repository/merge_base`;
  URL forms `/-/tree/<ref>`, `/-/commit/<sha>`, `/-/compare/A...B`, `/-/merge_requests/<n>`;
  CORS of gitlab.com, with and without a token, still to be measured.
- Not done: OAuth; a GitHub token (`Net.token` stays unused: GitHub needs it on two hosts,
  the API and the raw host, and the raw host was not measured with a preflight); Gitea
  instances were not tested, only Forgejo; a private repository on a real instance was not
  tested.
