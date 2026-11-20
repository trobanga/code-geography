# 40. The commits of a local repository and their overlay

Status: accepted (2026-11-20)

## Context
ADR 0039 reads the `.git` of a chosen directory, but only for the overlay of the working tree
against `HEAD`. A repository of a forge has a commit list, the overlay of one commit against its
first parent and the overlay of a range (ADR 0021). The commits of a local repository lie in the
bytes that the worker already holds, so the same should work without a forge. The index of a
mid repository takes seconds, and the packs can take hundreds of MB.

## Decision
- **The repository stays in one worker.** Opening a directory with a usable `.git` starts a
  worker for it (`localRepoWorker` in `web/src/lib/indexProject.ts`) that lives as long as the
  directory is shown. The bytes of `.git` are transferred to it once; there they become one
  `GitRepo` (crates/wasm). Every later question (a page of the list, a sha, an overlay) is a
  message to that worker: no read of `.git`, no copy of the packs per click. Another source
  (another directory, a forge, a file, `/graph.json`) ends the worker and frees its memory. The
  plain index and the overlays of a forge keep their one-shot workers.
- **The list walks the first parents from `HEAD`**, 30 commits a page ("load more commits"
  goes on from the first parent of the last commit listed); the whole history is never read.
  As for a forge, the commits of update bots and their merges are left out of the rows, and a
  listing reads up to five pages until 30 rows stay (fts-next: most of its first parents are
  Renovate merges, one page gave 5 rows).
  Side branches of a merge are not listed; a merge commit is listed and, like every commit, is
  compared with its first parent, as `rangeOfCommit` does for a forge. The list is the forge's
  `CommitList` with three optional parts: a first row for the uncommitted changes, a range
  `A..B` typed into its field, and the text of the hint. A sha typed into the field that the
  list does not hold (7 to 40 hex digits) is resolved by the reader: a unique abbreviation among
  the loose objects and the pack indexes.
- **A chosen commit shows the repository at that commit**: the overlay of the inputs of its tree
  against those of its first parent's tree (a root commit against nothing), as for a forge.
  `A..B` compares the tree of A with that of B; `A...B` (merge base) is not read. The changed
  files are all files whose blob differs between the two trees, inputs or not; a subtree with
  the same id on both sides is not read. Names: the short shas, `none` for the base of a root
  commit.
- **The source view of a chosen commit** reads the texts of both sides that the overlay returns
  (`textReader` with `head`, `base` and the names of the two commits): the lines of the commit,
  with its added and removed lines marked. The working tree texts stay in memory for the
  uncommitted entry.
- **The uncommitted changes stay one entry**, the first row of the list. Its overlay is kept
  from the open, so choosing it again indexes nothing.
- **Index cost.** The worker keeps the indexed inputs of the trees of the last overlay (at most
  two: graph, report, texts). A side whose tree is kept is not indexed again; two sides with the
  same tree are indexed once. A click on a commit costs two indexes in general, one when the next
  commit down the list is chosen (its head is the base of the overlay before) or right after the
  open (the uncommitted overlay indexed `HEAD`), none for the overlay shown or the uncommitted
  entry. `cg_index::diff_indexed` joins two indexed sides; `diff_sources` uses it. The cache key
  is the tree id with the root name. A root commit indexes the empty base too (no cost worth a
  rule). Measured in headless Chromium on fts-next (309 inputs, dev server): an overlay with two
  indexes 414 to 439 ms in the worker, with one 245 ms, the uncommitted entry 1 ms; the open of
  the directory (reading 67 MB of `.git` through the fallback input, the uncommitted overlay)
  15 s.
- **Commit data.** `cg-git` gives a commit its author name, its author date (ISO 8601 in the
  author's offset, as `git log --format=%aI`) and its message; the WASM answer has the shape of
  `Commit` of `forge.ts`, so the forge's `commitRows` and `rangeOfCommit` take it as is. The
  message encoding header is not read (UTF-8, invalid bytes replaced). The author name and
  the date are read from the author header by hand, not through `gix-actor` and `gix-date`:
  measured on the WASM module (profile `wasm`), the formatter of `gix-date` (jiff) added
  199 kB and the signature parser of `gix-actor` 51 kB. The module grows from 7 734 609 to
  7 825 824 bytes (+91 kB, 1.2 %; with both crates it was 8 073 871). The date is not
  compared with git before 1970, since `git commit` refuses such a date.
- **No commit context** (ADR 0030): a local repository has no forge, no pull requests and no
  web links.
- **No hash state.** A local directory is not in the link: a reload returns to the start screen,
  so the chosen commit is not written to the hash either.

## Consequences
- The worker holds the repository (packs, loose objects) plus up to two indexed trees for as long
  as the directory is shown; the 512 MiB limit of ADR 0039 still bounds the objects.
- No branch menu: the list is that of `HEAD`. A detached `HEAD` lists the history of its commit.
- A shallow clone: the walk ends at the first parent that no pack and no loose object holds;
  the overlay of the last commit listed fails with the missing object. `.git/shallow` is not
  read.
- A sha outside the first-parent history (a commit of a merged side branch) opens through the
  text field, not through the list.
