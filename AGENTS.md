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
| fact level | A node kind that comes from the language or build system: `module` (unit of build: Maven module, Cargo package, `deps.edn` directory), `package` (namespace: Java package, Rust `mod`, Clojure namespace; nests in all three, ADR 0018), `type`, `function`, `value` (a Clojure `def`, ADR 0027). |
| group | A container that does not come from the language (directory or AI). Not built yet. |
| edge | A dependency that the indexer can prove from the source: `extends`, `implements`, `imports`, `type_ref`, `calls` (ADR 0014), `value_ref` (ADR 0027). |
| cut | The set of visible nodes for a given set of expanded containers, with each edge moved to the nearest visible ancestors of its ends. |
| view | The result of a cut: visible nodes, nodes that can open, aggregated edges with counts. |
| focus cut | A cut that folds the children that are not of interest into one placeholder `rest:<container id>` (ADR 0011). |
| kind fold | A focus cut option (`fold_mixed`): a container with packages and other children, none of interest, shows the packages and folds the rest into `rest:<container id>` (ADR 0019). The viewer always sets it; a click on the placeholder or "+" puts the container into `unfolded`. |
| overlay | The diff graph of a commit: the union of two graphs with `change` marks, plus the list of changed files (ADR 0009, 0012). |
| implied line | A line A→C of a view for which the drawn lines hold a path from A to C through another box. The viewer can hide these (`hide implied`, ADR 0026); the graph keeps the edge. |
| front end | The part that turns the sources of one language into a graph (Java, Rust, Clojure). `cg-index` runs all of them. |
| test code | Code in a source set other than `src/main`; for Clojure, code in a directory that only an alias of `deps.edn` names (ADR 0023). Hidden by default. |

## Layout

```
crates/core        graph model, cut, filters. No I/O. Builds for native and wasm32.
crates/index-java  Java front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (pure, builds for wasm32); index(root) walks a directory and calls it
crates/index-rust  Rust front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (Cargo packages, mod tree, ADR 0016, call edges, ADR 0017); no walk
crates/index-clojure  Clojure front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (deps.edn modules, namespaces as packages, imports edges, ADR 0023; call
                   edges, ADR 0024; values, defmethod and extend-protocol nodes, ADR 0027); Layout::test_code(path) for the
                   changed files of an overlay
crates/walk        cg-walk: the one directory walk, read_sources(root, is_input) -> Vec<SourceFile>
                   (the only place with std::fs besides the CLI; used by index-java and index)
crates/index       cg-index: is_input, index_sources, index(root) over all front ends; joins
                   the graphs and reports (a repository can hold several languages; Java in a
                   deps.edn module and one package tree per module, ADR 0025)
                   diff_sources(root_name, base, head, DiffSpec): the overlay of two source sets
                   (CLI and WASM share it)
crates/cli         cg index <repo> -o graph.json; cg diff <repo> <rev|A..B> -o diff.json
crates/wasm        wasm-bindgen binding of core and the indexers: class Explorer,
                   indexSources(paths, texts, rootName), diffSources(...), isInput(path)
scripts/build-wasm.sh   builds crates/wasm into web/src/lib/wasm (generated, committed);
                        sets the C include path for the tree-sitter grammars (needs cargo, jq)
web/               viewer: Svelte 5, Tailwind 4, ELK layout in a web worker, plain SVG
web/src/lib/forge.ts   opens a repository of a forge: pool, cache by blob sha, progress (ADR 0020);
                   github.ts and forgejo.ts are the Forges (URLs, answers, errors, URL forms);
web/src/lib/forges.ts  kind of forge (github, forgejo, later gitlab): kindOf(host), forgeFor(kind, ...),
                   parseSource; hash gh=owner/repo@sha, forge=kind:host/owner/repo@sha (ADR 0022)
web/src/lib/tokenStore.ts  tokens by origin (session/local storage) and authorized(): the only
                   place that adds a token, refuses any other origin (ADR 0022)
docs/adr/          architecture decision records (0016: how Rust maps onto the graph, 0023: Clojure)
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
- `/home/trobanga/development/samply/blaze` — Clojure, 73 `deps.edn` modules, 1047 `.clj`
  files, 114 Java files, 199 000 lines. Read only. Use it for the Clojure front end and
  for the viewer at scale.

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
- No AI grouping (deferred, ADR 0010). GitHub: public repositories only (no token yet,
  60 API calls per hour); the overlay from a forge compares trees by blob sha (ADR 0021).
  The branch menu lists all branches (one API call per 100, on each open of the menu), in
  alphabetical order, not by age; the hash holds the sha only, so a
  reload shows the same commit but no longer knows the branch name.
  The text field of the commit list filters the loaded commits only ("load more commits"
  gets the next 30); a pasted sha is resolved by the forge, also outside the branch shown.
- Forgejo (ADR 0022): an unknown host is taken as Forgejo (`kindOf`); GitLab is
  planned, not built ("not supported yet"). A private
  repository needs a token and `[cors] ENABLED = true` with the viewer origin in
  `ALLOW_DOMAIN` on that host. The token path ran only against a mock. One API call per
  file (Codeberg: seconds per file). "forget token" clears the text cache of all hosts.
  No Forgejo under a sub-path; no OAuth; no CSP (the host is free input).
- Clojure (ADR 0023, 0024): an unqualified name gives no edge if its top-level form
  could bind it anywhere (7 % of such call heads on blaze). A static method call of a
  Java class goes to the type, a type hint gives no edge (ADR 0025).
  Dispatch of protocols and multimethods is not followed (`implements` edges show the
  methods, ADR 0027). No node for what a macro of the repository declares.
  Java and Rust make no value nodes. No `.cljs`, no Leiningen.
  Duplicate namespace names: the first file wins.
  The grammar crate is pinned to 0.2.5 (0.2.9 does not build for wasm32).
- Local classes and members of anonymous classes get no node.
- A rename of a type or method looks like one removal and one addition.
