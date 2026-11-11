# 20. A public GitHub repository opens in the browser

Status: accepted (2026-11-11)

## Context
The viewer is a static page; it indexes sources in the browser (ADR 15). A user wants to
look at a GitHub repository without a checkout, a server or a login. Measured on
2026-11-11:
- `api.github.com` allows cross-origin reads (`access-control-allow-origin: *`) and exposes
  `x-ratelimit-remaining` and `x-ratelimit-reset`. Without a token: 60 requests per hour
  per IP address.
- `GET /repos/{o}/{r}/commits/{ref}` with `Accept: application/vnd.github.sha` gives the
  commit sha as plain text; `HEAD` is the default branch; a ref with `/` works encoded as
  `%2F`; an unknown ref gives 422.
- `GET /repos/{o}/{r}/git/trees/{sha}?recursive=1` gives all paths with blob shas in one
  call, with `truncated: true` for very large repositories.
- `raw.githubusercontent.com/{o}/{r}/{sha}/{path}` gives one file, allows cross-origin
  reads and does not count against the API limit.
- The zip download (`codeload.github.com`) has no CORS header, so a browser cannot read it.

## Decision
- `web/src/lib/github.ts`: resolve the ref to a commit sha (one call, none for a full sha),
  list the tree (one call), keep the paths that the WASM `isInput` accepts, fetch their
  texts from the raw host, 10 at a time, then index in the worker as a local directory.
  `fetch`, the cache and a future token are parameters.
- File texts are cached by blob sha in IndexedDB (memory when IndexedDB is not usable).
  A blob sha names its content, so an entry never goes stale and is shared between
  commits. The tree of a commit is cached by commit sha for the same reason.
- The URL hash holds `gh=owner/repo@<commit sha>`, not the ref as typed: a reload or a
  shared link shows the same commit, and costs no API call when the cache is warm.
- A truncated tree is refused: a part of the files would give a graph that looks whole.
- No zip, no server, no login.

## Consequences
- At most two API calls per open, so about 30 opens per hour without a token. A reload
  of a cached commit needs none.
- The files come one request each; the first open of a 300-file repository takes about
  9 s, a cached one well under 1 s (measured with fts-next in Firefox).
- A link with a sha does not follow its branch; to see a newer commit, open the
  repository again.
- A private repository needs a token (`Authorization` header on both hosts), which is
  not built yet. The commit overlay (ADR 9) can reuse the fetch layer and the cache with
  the WASM `diffSources`.
