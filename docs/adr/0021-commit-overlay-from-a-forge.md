# 21. The commit overlay of a forge repository is built in the browser from two trees

Status: accepted (2026-11-11)

## Context
A repository opened from GitHub (ADR 20) should show the commit overlay (ADR 9, 12)
without the CLI. The browser has no git: it has the REST API (60 calls per hour without
a token), the raw host (no limit) and a cache of texts by blob sha. The WASM module has
`diffSources`, which takes both source sets and the list of changed files.

## Decision
- Changed files come from a compare of the two trees by blob sha: only in head is
  `added`, only in base `removed`, in both with another blob `modified`; sorted by path,
  which is the order of `git diff`. No rename detection, as `cg diff --no-renames`. All
  changed paths go to `diffSources`; only indexer inputs are fetched. No diff API: the
  GitHub compare lists at most 300 files, and the trees are needed anyway.
- The head is fetched first, the base second, so each unchanged file of the base is a
  cache hit by blob sha. Both sets are indexed by `diffSources` in the worker.
- `Forge` gets `commits(repo, sha, n)`, `mergeBase(repo, a, b)` and `pull(repo, n)`.
  The commit list of a sha is cached like a tree: the history of a commit never changes.
- A commit is compared with its first parent; a root commit with nothing (base `none`).
  `A..B` compares A with B, `A...B` and a pull request the merge base with the head.
- The hash holds full shas: `gh=owner/repo@<head>&base=<base sha|none>`. A reload or a
  shared link costs no API call when both trees are cached.
- The plain graph of the head is derived from the overlay (drop removed nodes and edges
  and the marks), so leaving the overlay neither fetches nor indexes again.

## Consequences
API calls per form, trees uncached (each cached tree saves one):
- `.../commit/<sha>`: 3 (history of 1 for sha and parents, 2 trees).
- `owner/repo@A..B`: 2 trees plus one per end that is not a full sha.
- `.../compare/A...B`: 2 trees, 1 merge base, plus 1 when B is not a full sha.
- `.../pull/<n>`: 4 (pull request, compare for the merge base, 2 trees).
- Commit list: 1, cached; a picked commit: up to 2 trees.
- Measured on fts-next (commit 7f1d05b, a merge, 18 changed files): equal to `cg diff` in
  nodes and edges per mark and in `diff.files`; cold 10.6 s, reload 0.6 s with 0 calls.
- A change of the file mode only (same blob) and a changed submodule are not seen; the
  CLI lists them as `modified`. `export-ignore` of `.gitattributes`, which `git archive`
  applies in the CLI, does not apply here.
- `diff.base`/`diff.head` are short shas, where the CLI writes the revisions as given.
