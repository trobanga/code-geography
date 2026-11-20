# 39. The viewer reads the git history of a local directory

Status: accepted (2026-11-20)

## Context
A local directory was indexed as it is on disk. The overlay of a commit (ADR 0009, 0012)
existed only for a repository of a forge (ADR 0021) and for `cg diff`. The browser reads no
`.git`, so the developer could not see what the uncommitted changes touch. The user chose to
read the git objects in WASM with `gix` crates, not with a JavaScript library.

## Decision
- A crate `cg-git` (`crates/git`) reads a repository from bytes in memory: `HEAD`, the files
  below `refs`, `packed-refs`, packs with their indexes, loose objects. No file system; it
  builds for `wasm32-unknown-unknown`. It uses `gix-pack` 0.77, `gix-object` 0.66, `gix-hash`
  0.28 and `gix-zlib` 0.4 without default features (SHA-1 only). Its interface is what an
  overlay needs: `resolve` (`HEAD`, a full ref name or an id; symbolic refs are followed, a
  loose ref wins over a packed one), `commit` (tree and parents), `files` (the blobs below a
  tree that a predicate wants), `blob`. CG-14 builds the commit list on it.
- A loose object is inflated when it is read. A delta base must be in the same pack: git
  stores no thin pack (`index-pack --fix-thin` completes it), so a base elsewhere is an
  error, not a lookup.
- The viewer reads `.git` of the chosen directory in TypeScript (`readGit` in
  `web/src/lib/localGit.ts`), from the picker, a drop, or the files of the fallback
  `<input webkitdirectory>`. That fallback has listed every file below `.git` already (as it
  did before); `filesDir` makes a directory of those below `.git` and the same reader reads
  them. Read: `HEAD`, all of `refs`, `packed-refs`, each `.idx` with its `.pack`, every loose
  object. Not read: `config`, `index`, `objects/info/alternates`, `.rev` and `.keep` files.
- Limit: the packs and loose objects together may take 512 MiB (`MAX_OBJECT_BYTES`). The
  sizes are known before a file is read; above the limit nothing of `objects` is read and the
  directory is indexed without overlay, with a note. Reason: `gix_pack::data::File::from_data`
  needs the whole pack in WASM memory; the worker holds the bytes once as read and once in
  WASM, beside two indexes of the sources, and wasm32 memory ends at 4 GiB (less on some
  browsers). Reading slices on demand would need a pack reader of our own, since the
  `from_data` constructors take the whole file. Measured: blaze 62 MB of packs, fts-next
  19 MB, this repository 33 MB of loose objects.
- The bytes move to the index worker (transferred, not copied). There `GitRepo` (crates/wasm)
  takes them and `worktreeOverlay` builds the overlay: the inputs at `HEAD` are the base, the
  texts that the walk read are the head, joined by `cg_index::diff_sources` like every other
  overlay. The base takes the files of the tree that the walk would read: a directory is read
  when the rule of the indexer wants a source file in it (as `walk.ts` asks), a file when
  `is_input` wants it, so a tracked `target/` or `node_modules/` is neither inflated nor
  shown as removed. Names: base = the short sha of `HEAD`, head = `working tree`.
- A file is changed when its text differs. A blob is decoded as the browser decodes a file
  (UTF-8, invalid bytes replaced, a byte order mark dropped). An input that `HEAD` lacks is
  `added`, also when git does not track it: the walk reads the directory, `.gitignore` is
  not read. An input at `HEAD` that the working tree lacks is `removed`.
- The overlay also returns the texts at `HEAD`; the source view of the local directory
  (`textReader` with `base`, CG-12) marks the uncommitted lines with them.
- No overlay, but the plain index with a note in the project bar:
  - `.git` is a file (a work tree or a submodule): it points outside the chosen directory,
    which the browser may not read. The note names the target and says to open the main
    repository.
  - `.git` has no `HEAD`, a file of it cannot be read (git removed it after the listing),
    the objects are over the limit, or git cannot give the commit at `HEAD` (no commit yet,
    a missing object, objects behind `alternates`). A file below `objects/xx` whose name is
    not 38 hex digits (`tmp_obj_…` of a crashed commit) is skipped.
  A directory with no `.git` is indexed as before.
- A clean working tree is indexed once: `cg_index::diff_sources` joins the graph with itself
  when the two source sets are equal (same paths, same texts, any order), which gives the
  overlay of two indexes, since the index does not depend on the order of the files. The
  result has the same shape (`diff` with base, head and no files; `base` texts). Measured on
  fts-next (309 inputs): 349 ms → 183 ms. With uncommitted changes both sides are indexed.
- The WASM module is built with the Cargo profile `wasm` (`opt-level = 2`, LTO,
  `panic = "abort"`, `strip`), which only `scripts/build-wasm.sh` uses; the native CLI keeps
  the release profile (`opt-level = 3`, no LTO). The user pays the index time on each open
  and the download once, so the profile must not slow the index. Measured with this change
  under Node, `indexSources` on fts-next (309 inputs) and on this repository (168 inputs):

  | profile | bytes | gzip | fts-next | this repository |
  |---|---|---|---|---|
  | release | 8 377 759 | 1 263 954 | 173 ms | 4.14 s |
  | `wasm`, `opt-level = 2` (chosen) | 7 734 609 | 1 186 139 | 177 ms | 4.14 s |
  | `wasm`, `opt-level = "s"` (rejected) | 7 386 896 | 1 075 850 | 201 ms | 4.99 s |

  `opt-level = 2` is 7.7 % smaller (gzip 6.2 %) at the speed of the release profile.
  Rejected: `opt-level = "s"`, 4.5 % smaller again (gzip 9.3 %), but the index is 16 % to
  21 % slower. Before this change the module was 8 074 250 bytes: `cg-git` adds about 300 kB
  at the release profile.

## Consequences
- An uncommitted change shows as an overlay, with the commit strip, `changes only` and the
  marks of the source view, as a commit of a forge does.
- Every loose object is read, not only those that `HEAD` needs: git packs them at 6700
  (`gc.auto`), and reading them all keeps the reader synchronous. A repository with many
  large loose objects meets the limit sooner.
- `core.autocrlf`: a working tree with CRLF line ends against LF blobs shows every such file
  as modified; filters (`.gitattributes`, LFS) are not applied.
- SHA-256 repositories are not read (`sha1` only): their object names are skipped or their
  packs refused, and the note gives the error of the reader.
- A work tree of this repository's agents (`.claude/worktrees/…`) gives the note, not an
  overlay.
- Only the overlay against `HEAD`: the commit list and the overlay of a chosen commit of a
  local repository are CG-14 (ADR 0040).
