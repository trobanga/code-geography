# code-geography

A tool that shows the structure of a code base in the browser. The code base is a
tree of containers with dependency edges. The developer zooms by opening single
containers, and a commit can be laid over the picture to show what it touches.

Decisions and their reasons are in `docs/adr/`. Read them before you change the
model. Record each new decision of that weight as a new ADR.

## Vocabulary

| Term | Meaning |
|---|---|
| graph | Nodes in a tree (`parent`) plus edges between any two nodes. |
| fact level | A node kind that comes from the language or build system: `module` (unit of build), `package` (namespace), `type`, `function`. |
| group | A container that does not come from the language (directory or AI). Not built yet. |
| edge | A dependency that the indexer can prove from the source: `extends`, `implements`, `imports`, `type_ref`. |
| cut | The set of visible nodes for a given set of expanded containers, with each edge moved to the nearest visible ancestors of its ends. |
| view | The result of a cut: visible nodes, nodes that can open, aggregated edges with counts. |
| focus cut | A cut that folds the children that are not of interest into one placeholder `rest:<container id>` (ADR 0011). |
| overlay | The diff graph of a commit: the union of two graphs with `change` marks, plus the list of changed files (ADR 0009, 0012). |
| front end | The part that turns the sources of one language into a graph. |
| test code | Code in a source set other than `src/main`. Hidden by default. |

## Layout

```
crates/core        graph model, cut, filters. No I/O. Builds for native and wasm32.
crates/index-java  Java front end on tree-sitter: index(root) -> Graph
crates/cli         cg index <repo> -o graph.json; cg diff <repo> <rev|A..B> -o diff.json
crates/wasm        wasm-bindgen binding of core: class Explorer
scripts/build-wasm.sh   builds crates/wasm into web/src/lib/wasm (generated, committed)
web/               viewer: Svelte 5, Tailwind 4, ELK layout in a web worker, plain SVG
docs/adr/          architecture decision records
```

## Commands

```
cargo test --workspace
cargo clippy --workspace --all-targets      # must be free of warnings
cargo fmt --all --check
cargo build -p cg-core --target wasm32-unknown-unknown   # core must stay free of I/O
scripts/build-wasm.sh                        # after each change in crates/core or crates/wasm
cargo run --release -p cg -- index <repo> -o web/public/graph.json

cd web
pnpm check && pnpm test && pnpm build
pnpm dev                                     # http://localhost:5173/
```

## Rules

- Test first: one failing test, minimal code, repeat. Never several tests before the code.
- Tests go through public interfaces: `index()` on a temporary source tree, `Graph::cut`,
  the exported TypeScript functions. No tests of private functions.
- The indexer makes edges only from what it can resolve. No guessed edges (ADR 0001).
- Node ids come from qualified names only, never from positions or file paths (ADR 0005).
- `crates/core` has no I/O and no dependency that needs it.
- In the viewer, logic goes into pure functions in `web/src/lib` with Vitest tests. The cut
  is done in WASM and is not reimplemented in TypeScript.
- The `wasm-bindgen` crate version in `crates/wasm/Cargo.toml` must be equal to the version
  of the installed `wasm-bindgen` CLI.
- `svelte-check` needs TypeScript 6. Do not move to TypeScript 7 before it supports it.

## Test repositories

- `/home/trobanga/development/mii/fts-next` — Java, 6 Maven modules, 300 files. Read only.
  Small and tidy: good for the indexer and the viewer, too tidy to judge AI grouping.
- `/home/trobanga/code/dd-empire/momenthafen/api` — Rust. Needs the Rust front end.

## Known gaps

- No call edges. A static call `Foo.bar()` gives no edge.
- No Rust front end, no AI grouping, no GitHub connection.
- Local classes and members of anonymous classes get no node.
- A rename of a type or method looks like one removal and one addition.
