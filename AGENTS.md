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
| fact level | A node kind that comes from the language or build system: `module` (unit of build: Maven module, Cargo package), `package` (namespace: Java package, Rust `mod`), `type`, `function`. |
| group | A container that does not come from the language (directory or AI). Not built yet. |
| edge | A dependency that the indexer can prove from the source: `extends`, `implements`, `imports`, `type_ref`, `calls` (ADR 0014). |
| cut | The set of visible nodes for a given set of expanded containers, with each edge moved to the nearest visible ancestors of its ends. |
| view | The result of a cut: visible nodes, nodes that can open, aggregated edges with counts. |
| focus cut | A cut that folds the children that are not of interest into one placeholder `rest:<container id>` (ADR 0011). |
| overlay | The diff graph of a commit: the union of two graphs with `change` marks, plus the list of changed files (ADR 0009, 0012). |
| front end | The part that turns the sources of one language into a graph (Java, Rust). `cg-index` runs all of them. |
| test code | Code in a source set other than `src/main`. Hidden by default. |

## Layout

```
crates/core        graph model, cut, filters. No I/O. Builds for native and wasm32.
crates/index-java  Java front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (pure, builds for wasm32); index(root) walks a directory and calls it
crates/index-rust  Rust front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (Cargo packages, mod tree, ADR 0016, call edges, ADR 0017); no walk
crates/walk        cg-walk: the one directory walk, read_sources(root, is_input) -> Vec<SourceFile>
                   (the only place with std::fs besides the CLI; used by index-java and index)
crates/index       cg-index: is_input, index_sources, index(root) over all front ends; joins
                   the graphs and reports (a repository can hold Java and Rust)
crates/cli         cg index <repo> -o graph.json; cg diff <repo> <rev|A..B> -o diff.json
crates/wasm        wasm-bindgen binding of core and the indexers: class Explorer,
                   indexSources(paths, texts, rootName), isInput(path)
scripts/build-wasm.sh   builds crates/wasm into web/src/lib/wasm (generated, committed);
                        sets the C include path for the tree-sitter grammars (needs cargo, jq)
web/               viewer: Svelte 5, Tailwind 4, ELK layout in a web worker, plain SVG
docs/adr/          architecture decision records (0016: how Rust maps onto the graph)
```

## Commands

```
cargo test --workspace
cargo clippy --workspace --all-targets      # must be free of warnings
cargo fmt --all --check
cargo build -p cg-core --target wasm32-unknown-unknown   # core must stay free of I/O
scripts/build-wasm.sh                        # after each change in crates/core, index-java or wasm
cargo run --release -p cg -- index <repo> -o web/public/graph.json

cd web
pnpm check && pnpm test && pnpm build
pnpm dev                                     # http://localhost:5173/
```

## Rules

- Test first: one failing test, minimal code, repeat. Never several tests before the code.
- Tests go through public interfaces: `index()` on a temporary source tree (Rust:
  `index_sources` on a few `(path, text)` pairs), `Graph::cut`, the exported TypeScript
  functions. No tests of private functions.
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
- `/home/trobanga/code/dd-empire/momenthafen/api` — Rust, one crate, 178 files, DDD layout
  with many `mod`s. Read only. Use it for the Rust front end.

## Known gaps

- Call edges (ADR 0014) need a receiver type that the declarations prove. No edge for a
  chain through an external type, a lambda parameter without a written type, a `var` that
  is not `var x = new T()`, members that Lombok generates, or overloads that differ only
  by type (the edge then goes to the type). `cg index` prints how many call sites fell
  into each class.
- Rust call edges (ADR 0017) need a type that the declarations prove: a parameter, a
  `let` with a written type or a unique function behind it, a field of a repository struct.
  On a 178-file async service 27 % of the call sites give an edge, 35 % are external and
  38 % unresolved (results of external calls, `let` with an inferred type, iterator chains). Calls in macro arguments (`format!`, `vec!`,
  `assert!`, `sqlx::query!`) are not read; the report counts them roughly
  (`macro_call_sites`). Calls inside the root mod of a crate give no edge, because the
  module node is the ancestor of everything in the crate.
- Rust: items that macros generate get no node; files that no
  `mod` reaches and `#[path]` mods are missing (the report lists them); `[lib]` and
  `[[bin]]` paths other than the Cargo defaults are not read (ADR 0016).
- No AI grouping, no GitHub connection.
- Local classes and members of anonymous classes get no node.
- A rename of a type or method looks like one removal and one addition.
