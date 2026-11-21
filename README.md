# code-geography

code-geography shows the structure of a code base in the browser. It reads the
sources of a repository and makes a graph: a tree of containers (modules, packages,
types, functions), plus the dependency edges between the nodes (`extends`,
`implements`, `imports`, `type_ref`, `calls`, `value_ref`). The viewer draws one
*cut* of this graph: the nodes that are visible for the containers you opened, with
each edge moved to the nearest visible ancestors of its two ends and counted there.
To zoom in, open one container at a time.

You can lay a commit over the picture. The overlay is the union of the graphs of two
revisions, with each node and edge marked as added, removed or kept, plus the list of
changed files.

The indexer makes an edge only if it can resolve the reference from the source.
It does not guess (`docs/adr/0001`). As a result, some edges are missing, for example
calls through a type that the declarations do not prove. `AGENTS.md`, section
*Known gaps*, lists these limits for each language.

## Languages

| Language | Build unit (`module`) | Namespace (`package`) |
|---|---|---|
| Java | Maven module | Java package |
| Rust | Cargo package | `mod` |
| Clojure | `deps.edn` directory | namespace |
| Elixir | Mix project | module (`defmodule`) |
| TypeScript | directory of a `package.json` | file and directory |

A repository can hold more than one language. All front ends use tree-sitter, and the
viewer runs them in the browser, compiled to WebAssembly.

## Install

You can use the viewer and the command-line tool `cg` separately.

### Viewer, in the browser

Open https://trobanga.github.io/code-geography/. This is the viewer of the latest
release, and it needs no installation. The indexers run in your browser. A local
directory that you open does not leave your machine. A forge repository is read
directly from the API of its host.

### Viewer, from a release

To run the viewer on your own host, for example with a `graph.json` that `cg` made:

1. Download `code-geography-vX.Y.Z-viewer.tar.gz` from the
   [releases](https://github.com/trobanga/code-geography/releases).
2. Extract it into an empty directory.
3. Serve that directory with a static server. A sub-path is correct (from v0.1.1):

   ```sh
   mkdir viewer && tar -xzf code-geography-v0.4.0-viewer.tar.gz -C viewer
   cd viewer && python3 -m http.server 8000
   ```

4. Open http://localhost:8000/.

### Command-line tool `cg`

`cg` needs Rust and a C compiler, because the tree-sitter grammars are C code. It was
tested with Rust 1.100. The crates use edition 2024, so Rust 1.85 is the minimum.

```sh
cargo install --locked --git https://github.com/trobanga/code-geography --tag v0.4.0 cg
```

### From the sources

```sh
git clone https://github.com/trobanga/code-geography
cd code-geography
cargo build --release -p cg           # target/release/cg
cd web && pnpm install && pnpm dev    # viewer at http://localhost:5173/
```

The viewer needs Node.js 20.19 or later and pnpm. CI uses Node.js 24 and pnpm 11. The
compiled WebAssembly is committed in `web/src/lib/wasm`, so `pnpm dev` needs no Rust.
To rebuild it after a change in `crates/`, run `scripts/build-wasm.sh`. That script
needs `clang`, `jq`, the target `wasm32-unknown-unknown`, and the `wasm-bindgen` CLI
at the version in `crates/wasm/Cargo.toml`.

## Use

### In the browser only

Open the viewer and do one of these steps:

- Open a local project directory, or drop it on the page. The browser reads the
  files and indexes them, and nothing goes to a server. A reload forgets the result.
- Give a repository of GitHub (public only) or of a Forgejo host (a private one needs
  a token). The viewer reads the files through the API of the forge. Then you can
  select a branch and a commit, lay the commit over the picture, and read the source
  lines of a box or of a line.
- Open a `graph.json` that `cg` made.

### With `cg`

```sh
cg index <repo> -o graph.json        # the graph of a working tree
cg diff <repo> <rev> -o diff.json    # the overlay of <rev> against its parent
cg diff <repo> A..B -o diff.json     # the overlay of B against A
```

`cg diff` reads the revisions with `git`. `cg index` and `cg diff` print a report on
stderr: for example, how many call sites gave an edge, how many were external, and
how many were not resolved.

Put the graph as `graph.json` next to the `index.html` of the viewer: the viewer
loads it at the start. Put the overlay next to it as `commit.json`: then the toolbar
lets you switch between the two. For `pnpm dev`, the directory is `web/public/`.

## Documentation

- `AGENTS.md`: the vocabulary, the layout of the crates, the commands, the rules for
  contributions, and the known gaps.
- `docs/adr/`: the architecture decisions and their reasons.

## License

Apache License 2.0, see `LICENSE`.
