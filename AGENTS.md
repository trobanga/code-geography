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
| fact level | A node kind that comes from the language or build system: `module` (unit of build: Maven module, Cargo package, `deps.edn` directory, Mix project, directory of a `package.json`), `package` (namespace: Java package, Rust `mod`, Clojure namespace, Elixir module, TypeScript file or directory, ADR 0034; nests in all five, ADR 0018), `type`, `function`, `value` (a Clojure `def`, ADR 0027). |
| group | A container that does not come from the language (directory or AI). Not built yet. |
| edge | A dependency that the indexer can prove from the source: `extends`, `implements`, `imports`, `type_ref`, `calls` (ADR 0014), `value_ref` (ADR 0027). One edge per `(from, to, kind)`, however many references make it. |
| site | A line of a file where a reference stands that makes an edge (ADR 0032). An edge has `sites`: entries `{file, lines}`; `file` is left out when it is the file of the `from` node. Data like `span`: in no id and not in the identity of an edge. In an overlay a removed edge has the sites of the base, every other edge those of the head. Not the "call site" of a report, which is each place of a call, resolved or not. |
| cut | The set of visible nodes for a given set of expanded containers, with each edge moved to the nearest visible ancestors of its ends. |
| view | The result of a cut: visible nodes, nodes that can open, aggregated edges with counts. |
| focus cut | A cut that folds the children that are not of interest into one placeholder `rest:<container id>` (ADR 0011). |
| kind fold | A focus cut option (`fold_mixed`): a container with packages and other children, none of interest, shows the packages and folds the rest into `rest:<container id>` (ADR 0019). The viewer always sets it; a click on the placeholder or "+" puts the container into `unfolded`. |
| overlay | The diff graph of a commit: the union of two graphs with `change` marks, plus the list of changed files (ADR 0009, 0012). |
| implied line | A line A→C of a view for which the drawn lines hold a path from A to C through another box. The viewer can hide these (`hide implied`, ADR 0026); the graph keeps the edge. |
| changes together | A viewer option on top of `changes only`: the layout gets the changed lines only and puts the active boxes of each container together, in `layout:` boxes that only the ELK input knows (ADR 0028). Off: the layout stays, the other boxes are dimmed. |
| selection | The boxes that the developer selected: a click selects one, Shift+click adds or removes one. Lines between two selected boxes are full, lines to the outside weak (dim with `between selected`); `open all` / `close all` works on all of them (ADR 0029). |
| commit context | For an overlay of a forge repository, on demand: the full message of each commit of the overlay, its pull requests and the issue references (`#12`, `owner/repo#12`) read from the message and the pull request bodies, as links. A reference is a guess (ADR 0030). |
| source view | The panel below the diagram for one or two selected boxes (ADR 0031). One box: its lines (`file`, `span`), or for a module or package the list of its declarations (the topmost nodes with a span below it). Two boxes: the declarations whose edges make their line, each with its targets, from `Explorer.edgesBetween`; an open declaration marks the lines of the sites of these edges and shows the sites outside its own lines below them (ADR 0032). Forge repositories only; a commit marks its added and removed lines. |
| front end | The part that turns the sources of one language into a graph (Java, Rust, Clojure, Elixir, TypeScript). `cg-index` runs all of them. |
| test code | Code in a source set other than `src/main`; for Clojure, code in a directory that only an alias of `deps.edn` names (ADR 0023); for Elixir, code below the directory `test` of a Mix project (ADR 0035); for TypeScript, a file that Jest and Vitest take as a test by its name (`*.test.ts`, `*.spec.ts`, below `__tests__` or `__mocks__`, ADR 0034). Hidden by default. |

## Layout

```
crates/core        graph model, cut, filters. No I/O. Builds for native and wasm32.
                   EdgeSet: the edges of a front end by (from, to, kind), with the sites of each (ADR 0032)
crates/index-java  Java front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (pure, builds for wasm32); index(root) walks a directory and calls it
crates/index-rust  Rust front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (Cargo packages, mod tree, ADR 0016, call edges, ADR 0017); no walk
crates/index-clojure  Clojure front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (deps.edn modules, namespaces as packages, imports edges, ADR 0023; call
                   edges, ADR 0024; values, defmethod and extend-protocol nodes, ADR 0027); Layout::test_code(path) for the
                   changed files of an overlay
crates/index-elixir  Elixir front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (Mix projects as modules, defmodule/defprotocol/defimpl as packages, def as
                   function `name/arity`, imports and implements edges, no call edges yet, ADR 0035);
                   scope.rs: the aliases of a lexical scope; Layout::test_code(path) as for Clojure
crates/index-typescript  TypeScript front end on tree-sitter: index_sources(root_name, files) -> Graph + Report
                   (package.json modules, files and directories as packages, imports, extends and
                   implements edges, ADR 0034; no call edges yet); resolve.rs: what the specifier of
                   an import reaches (relative paths, tsconfig paths, packages of the repository);
                   is_test_file(path) for the changed files of an overlay
crates/walk        cg-walk: the one directory walk, read_sources(root, is_input) -> Vec<SourceFile>
                   (the only place with std::fs besides the CLI; used by index-java and index)
crates/index       cg-index: is_input, index_sources, index(root) over all front ends; joins
                   the graphs and reports (a repository can hold several languages; Java in a
                   deps.edn module and one package tree per module, ADR 0025)
                   diff_sources(root_name, base, head, DiffSpec): the overlay of two source sets
                   (CLI and WASM share it)
crates/cli         cg index <repo> -o graph.json; cg diff <repo> <rev|A..B> -o diff.json
crates/wasm        wasm-bindgen binding of core and the indexers: class Explorer (view, focusView,
                   edgesBetween), indexSources(paths, texts, rootName), diffSources(...), isInput(path)
scripts/build-wasm.sh   builds crates/wasm into web/src/lib/wasm (generated, committed);
                        sets the C include path for the tree-sitter grammars (needs cargo, jq)
web/               viewer: Svelte 5, Tailwind 4, ELK layout in a web worker, plain SVG
web/src/lib/forge.ts   opens a repository of a forge: pool, cache by blob sha, progress (ADR 0020);
                   the interfaces Forge, CommitSource (between, pullsOf, links) and WebLinks;
                   github.ts and forgejo.ts implement them (URLs, answers, errors, URL forms,
                   the web links of the commit context: no other file builds a host URL);
web/src/lib/forges.ts  kind of forge (github, forgejo, later gitlab): kindOf(host), forgeFor(kind, ...), forgeOfRepo(repo),
                   parseSource; hash gh=owner/repo@sha, forge=kind:host/owner/repo@sha (ADR 0022);
                   `branch=` beside them (ADR 0033)
web/src/lib/commitContext.ts  the commit context (ADR 0030): issue references, the list of a range,
                   pull requests by sha, all links from the CommitSource; CommitContext.svelte
                   shows it under the commit strip
web/src/lib/source.ts  the source view (ADR 0031): sourceView (what a selection shows), lineGroups,
                   declarations, sourceReader (texts by blob sha from the cache, else the forge),
                   overlayLines (marks of a commit, from lineDiff.ts), readInto (the state of one read);
                   withRefs (the sites of edges on shown lines), readDeclaration (the lines of a
                   declaration and the sites outside them), openable (whether a row opens), ADR 0032;
                   SourcePanel.svelte, SourceList.svelte and SourceCode.svelte show it
web/src/lib/cover.ts  the cover of the diagram (the Survey picture) for a level button, "+" and "−": on the
                   screen before the cut of the step, off when its layout is there
web/src/lib/tokenStore.ts  tokens by origin (session/local storage) and authorized(): the only
                   place that adds a token, refuses any other origin (ADR 0022)
docs/adr/          architecture decision records (0016: how Rust maps onto the graph, 0023: Clojure, 0034: TypeScript, 0035: Elixir)
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

A release is a tag `vX.Y.Z` on `main`: CI builds the WASM and the viewer and makes a
GitHub release, and deploys the viewer to https://trobanga.github.io/code-geography/. The tag must equal the version of `Cargo.toml` (`[workspace.package]`)
and `web/package.json`. The skill `.agents/skills/cut-release` does the steps.

## Rules

- Test first: one failing test, minimal code, repeat. Never several tests before the code.
- Tests go through public interfaces: `index()` on a temporary source tree (Rust:
  `index_sources` on a few `(path, text)` pairs), `Graph::cut`, the exported TypeScript
  functions. No tests of private functions.
- The indexer makes edges only from what it can resolve. No guessed edges (ADR 0001).
- Node ids come from qualified names only, never from positions or file paths (ADR 0005).
  The one case with a path is TypeScript: a file has no name but its path in its module,
  so that path is its qualified name and stands in the ids (ADR 0034).
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
  alphabetical order, not by age. The hash holds the sha and the branch as a hint
  (`branch=`, ADR 0033): the name is not checked on a reload. A commit without a branch
  shows the default branch if it holds the commit (two API calls), else no branch; the
  viewer does not search the other branches. The commit list starts at the commit shown,
  not at the head of the branch.
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
  Java, Rust and TypeScript make no value nodes. No `.cljs`, no Leiningen.
  Duplicate namespace names: the first file wins.
  The grammar crate is pinned to 0.2.5 (0.2.9 does not build for wasm32).
- Elixir (ADR 0035): no call edges yet (CG-10), so a module that writes the names of
  other modules in full at the call shows no edge. What a `use` adds to a module is not
  known: its functions, imports, `@behaviour` and aliases. An alias that a `use` adds can
  hide a module of the repository: this is the one way to a wrong edge. No node for
  `defstruct`, `@type`, `@callback`, or for what a macro declares (`schema`, `test`); the
  body of a module has no hash. Definitions are read at the top level, in a module body
  and in `if`/`unless` blocks there, not in other blocks (`for`). A project is a
  `mix.exs`; sources are `lib` (or the string literals of `elixirc_paths`) and `test`.
  Not read: `config/`, `priv/` (migrations), `mix.exs` itself, files below a directory
  `deps` or `_build` at any depth, `.exs` outside `test`, a file with no `mix.exs` above
  it, a module that a test defines in its body. An `elixirc_paths` without a string
  literal counts as `lib`. A parent module that names its child shows no edge. A directive in a
  function is an edge of the module. A `defimpl` of a protocol outside the repository
  has no `implements` edge. Duplicate module names: the first form wins, main code
  before test code. The grammar adds 1.6 MB to the `.wasm` file.
- TypeScript (ADR 0034): no call edges yet (CG-8) and no `type_ref` edges: a file shows
  what it imports, a class what it extends and implements. `.ts` and `.tsx` only: no
  `.js`, `.jsx`, `.mts`, `.cts`, `.vue`, `.svelte`. A file with no `package.json` above it
  is not read (the report counts these; a Deno project stays empty). No node for a
  `const` that is not a function (an import of it goes to the package of its file), for
  a function that is the argument of a call (`memo(function A() {})`), for a declaration
  inside a function, for `declare module "x"` and `declare global`. `require` is not
  read. An alias resolves only through `paths` and `baseUrl` of a `tsconfig` in the
  repository or through the `name` of a `package.json`; an alias of a bundler
  (`resolve.alias`) or of a generated `tsconfig` (`.svelte-kit`, `.nuxt`) counts as
  unresolved. `include`, `exclude` and `files` of a `tsconfig` are not read. A package
  of the repository whose `package.json` names only built files (`dist/index.js`) gets
  the edge to its module, not to the declaration. Test code is found by the name of the
  file only (`*.test.ts`, `*.spec.ts`, `__tests__`, `__mocks__`), not by a directory
  `test`. The `index.ts` of a module directory is the module: its imports of files of
  the module give no edge. Of a merged declaration (two `interface A`) only the first
  part gives `extends` edges. A local directory is read with its built `.d.ts` files
  (`dist`). The grammar is of 2024 (`export type * from` is read as
  `export *`; a decorator before an overload signature is a syntax error) and adds
  3.2 MB to the `.wasm` file (two languages, `.ts` and `.tsx`).
- Commit context (ADR 0030): forge repositories only, not a local `diff.json`. Forgejo
  `commits/{sha}/pull` finds only the pull request whose merge commit is the sha; a commit
  inside a merged branch shows none. GitHub lists the newest 50 to 100 commits of a
  longer range (two calls), Forgejo what one compare answer holds. After a reload the
  context of a commit costs one compare call, as the hash does not hold the commit; a
  commit list cached before CG-4 has no messages, so its commits cost that call too.
  Issue references are read from the text, not checked; issues that a pull
  request links only on the forge do not show. Kept in memory only: a reload asks again.
  Both endpoints were checked against github.com and codeberg.org on 2026-11-16; Forgejo
  with a token was not.
- Source view (ADR 0031): no lines for a local graph, a local directory or `/commit.json`
  (lists only). No syntax highlighting. Under an active focus the list of a line can hold
  more edges than the count on the line. The marks of a commit follow a line diff of the
  file: moved code is removed and added lines; above 1000 changed lines in a file the part
  between the first and the last change counts as replaced. A span is rendered whole.
  A file that moved between the two commits of an overlay: the lines of its nodes show
  no marks of the commit, because the graph does not hold the path at the base (ADR
  0032). The same holds for a new file of a node that was there (a Rust `impl` in a new
  file): its lines are new, but not marked.
- Sites (ADR 0032): a site is a line, not a place in the line. A reference that a front
  end does not resolve has none, so an open declaration can hold uses that are not
  marked. A kept edge has the sites of the head only: a call that a commit removed while
  another call of the same target stays is a removed line without a mark of a reference.
  `merge imports` drops the import that a use covers, with its site. The view of one box
  marks nothing.
- Local classes and members of anonymous classes get no node.
- A rename of a type or method looks like one removal and one addition.
