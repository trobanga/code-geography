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
- One seam, `Forge { host, resolve, tree, text }` in `web/src/lib/forge.ts`. Only a forge
  knows URLs, answers and error messages of its host; `github(net)` in
  `web/src/lib/github.ts` is the only one, together with the parse of the input field.
  `forge.ts` knows no host: it pools the file requests (10 at a time), caches, reports
  progress and hands the inputs to the indexer. Private repositories will live on a
  self-hosted Forgejo (Gitea API); that is a second `Forge`, not a change of `forge.ts`.
- GitHub: resolve the ref to a commit sha (one call, none for a full sha), list the tree
  (one call), keep the paths that the WASM `isInput` accepts, fetch their texts from the
  raw host, then index in the worker as a local directory.
- File texts are cached by blob sha in IndexedDB (memory when IndexedDB is not usable).
  A blob sha names its content, so an entry never goes stale and is shared between
  commits and hosts. The tree of a commit is cached by host, repository and commit sha.
- The URL hash holds `gh=owner/repo@<commit sha>`, not the ref as typed: a reload or a
  shared link shows the same commit, and costs no API call when the cache is warm.
  `gh=` means the host github.com. Another host gets its own key with the host in it,
  `forge=<host>/<owner>/<repo>@<sha>`, because there is no default host to leave out.
- A token, when it comes, belongs to one `Forge` instance and so to one host; it is never
  part of the hash or of any URL.
- A truncated tree is refused: a part of the files would give a graph that looks whole.
- No zip, no server, no login.

## Consequences
- At most two API calls per open, so about 30 opens per hour without a token. A reload
  of a cached commit needs none.
- The files come one request each; the first open of a 300-file repository took 1.8 s
  and 9.3 s in two runs, a cached one under 0.5 s (fts-next in Firefox, through a proxy).
- A link with a sha does not follow its branch; to see a newer commit, open the
  repository again.
- A private repository needs a token, which is not built yet. Gitea pages its tree API,
  so a Forgejo `tree` makes more than one call; the interface hides that. The commit overlay of a forge repository is built
  in the browser on the same fetch layer and cache (ADR 21).
