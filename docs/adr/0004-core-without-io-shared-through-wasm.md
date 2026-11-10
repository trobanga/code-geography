# 4. A language-neutral core without I/O, shared through WASM

Status: accepted (2026-11-10)

## Context
The CLI and the browser need the same logic for the cut, the filters and the commit
diff. Two implementations would drift apart.

## Decision
`crates/core` holds the model and all graph operations. It does no I/O and builds for
native targets and for `wasm32-unknown-unknown`. `crates/wasm` exposes it to the browser
through wasm-bindgen as one class, `Explorer`, that takes and returns JSON. The viewer
does not reimplement graph operations in TypeScript. The indexer stays native for now.

## Consequences
- JSON crosses the WASM border on each view; for graphs of this size that cost is not
  measurable next to the layout.
- An indexer in the browser needs a C toolchain for WASM (tree-sitter is C). That work
  waits until the GitHub connection needs it.
- Update: the indexer runs in the browser too, see ADR 15.
