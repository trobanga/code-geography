# 15. The indexer runs in the browser

Status: accepted (2026-11-10)

## Context
The user opens a project directory in the browser, and the graph is made there. A
browser has no `std::fs`. ADR 4 kept the indexer native; the browser now needs it, and a
second indexer would drift from the first.

## Decision
- `cg-index-java` takes its input as data: `index_sources(root_name, &[SourceFile])` with
  `SourceFile { path, text }`, `path` relative to the root with `/`. The input is the
  `.java` files and the `pom.xml` files (only the place of a `pom.xml` counts, so its text
  may be empty). `is_input(path)` is the one rule for the files that the indexer wants (no
  hidden or `target` directory). The result does not depend on the order of the files.
- `index(root)` is the walk, `is_input`, reading, and then `index_sources`. The CLI and the
  browser run the same code; a graph from a directory is the same in both.
- `cg-wasm` exports `isInput(path)` and `indexJava(paths, texts, rootName)`, which returns
  the JSON `{ graph, report }`.
- tree-sitter is C. For `wasm32-unknown-unknown` the C compiler has no libc headers.
  `scripts/build-wasm.sh` points `CFLAGS_wasm32_unknown_unknown` at the `wasm/include`
  directory of the `tree-sitter-language` crate, which it finds through `cargo metadata`.
  No other toolchain is needed.

## Consequences
- The `.wasm` file grows from 0.29 MB to about 1 MB (gzip 0.10 MB to 0.25 MB).
- The browser must read all files itself and hand over paths and texts; the copy through
  the WASM border costs memory in proportion to the size of the sources.
- The commit overlay (ADR 9) still needs the CLI: a browser has no git, and the overlay
  is a diff of the graphs of two revisions.
- A new front end follows the same shape: sources as data in, a graph out.
