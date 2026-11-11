# 23. The Clojure front end: how Clojure maps onto the graph

Status: accepted (2026-11-11)

## Context
The third language is Clojure. The reference repository is `samply/blaze`: 1047 `.clj`
files, 199 000 lines, 73 `deps.edn` files (the root and 72 below `modules/`), 114 Java
files in `java` directories of seven of these modules, no `pom.xml`.

Clojure differs from Java and Rust in ways that decide the mapping:

- The grammar (`tree-sitter-clojure-orchard`, the sogaiu grammar) gives lists, vectors,
  symbols and keywords, nothing else. What a list means (`defn`, `ns`) is the work of the
  front end.
- A file holds one namespace, and the `ns` form names it. The path does not.
- A namespace holds functions directly, as a Rust `mod` does. `a.b` can be a namespace
  with functions and also the prefix of the namespace `a.b.c`.
- Macros of the repository declare things that no syntax tree shows (`defunop`,
  `def-resource-gen`: about 250 top-level forms in blaze).
- Where the test code is depends on `deps.edn`, not on a directory convention:
  `src/blaze/db/api.clj` is main code, and the rule of ADR 8 (`src/<not main>`) calls it
  test code.
- The same namespace name occurs in more than one module (`build` 16 times, in
  `build.clj` scripts that are on no classpath; `blaze.job.test-util` twice).

Stage 1 builds nodes and `imports` edges. Call edges and the other edges are stage 2 and
get an ADR of their own.

## Decision
- **Input**: `.clj`, `.cljc` and `deps.edn`. Not `.cljs`, `project.clj`, `bb.edn`,
  `shadow-cljs.edn`.
- **Module** (`mod:<dir>`): each directory with a `deps.edn`. Its name is the last
  segment of the directory, the root name for the root. A source file belongs to the
  nearest `deps.edn` above it.
- **Source directories**: the top-level `:paths` of the `deps.edn` (default `["src"]`),
  and the `:extra-paths` of each alias. `deps.edn` is parsed with the same grammar. A
  file is read only if it is below one of these directories of its module. So a
  `build.clj` beside the `deps.edn` gives no nodes; the report counts such files. A file
  below no `deps.edn` is not read.
- **Test code** (ADR 8): a file is test code if the longest of the source directories
  that contain it comes from an alias and not from `:paths`. The namespace node of such a
  file carries the marker too, unless a namespace of main code is below it, so that a
  file of `deftest` forms leaves no empty package. The changed-file list of an overlay
  uses the same rule for a `.clj` or `.cljc` file in a source directory of a `deps.edn`
  module (the `deps.edn` of the head, else of the base); other files keep the rule of
  ADR 8.
- **Package** (`pkg:<dir>:<namespace>`): each namespace, named by its `ns` form. They
  nest by the rule of ADR 18, per module, over the namespaces of that module. A file
  without an `ns` form gives no nodes; the report counts such files. In a `.cljc` file
  all branches of a reader conditional are read.
- **Function** (`fn:<namespace>/<name>`): `defn`, `defn-`, `defmacro`, `defmulti` at the
  top level of the file, children of the namespace. One node for all arities. The
  signatures of a `defprotocol` and of a `definterface` are functions below that type
  (`fn:<namespace>/<Type>#<name>`), and so are the methods in the body of a `defrecord`
  or `deftype`.
- **Type** (`type:<namespace>/<Name>`): `defprotocol`, `defrecord`, `deftype`,
  `definterface`, children of the namespace.
- **No node**: `def`, `defonce`, `declare`, `defmethod` (stage 2), `extend-protocol` and
  `extend-type` (stage 2), `s/def`, `s/fdef`, forms in a `comment`, forms that are not at
  the top level, and each top-level form whose head is not in the lists above. The
  report counts the top-level forms without a node by their head symbol.
- **Duplicate namespace**: if two files that are read name the same namespace, the first
  file in path order gives the nodes. The report lists the other files. The ids hold no
  module directory, as the type ids of Java.
- **Edges**, stage 1: `imports`, from the namespace to each namespace of the repository
  that its `ns` form names in `:require` or `:use` (plain symbol, vector with `:as` or
  `:refer`, prefix list). `:as-alias` does not load a namespace and gives no edge. No
  edge between a namespace and itself, its ancestor or its descendant.
- The ids contain `/` after a namespace, so they are disjoint from Java ids (`#`, `(`)
  and Rust ids (`::`). `mod:<dir>` and `pkg:<dir>:<name>` are shared on purpose.
- Own-text hash and span as for Java and Rust. The hash of a type leaves out its methods.
- The grammar crate is pinned to `tree-sitter-clojure-orchard` 0.2.5. The build script
  of 0.2.9 compiles libc stub files for `wasm32` that `tree-sitter-language` 0.1.9
  refuses for `tree-sitter` 0.27. The grammar of the two releases is the same.

## Measured on blaze
0.7 s for the index. 73 modules, 569 namespaces, 212 Clojure types and 4489 Clojure
functions outside test code; 5567 `imports` edges, which join 676 pairs of modules.
17 files are outside the source directories, 3 have no `ns` form, 1 namespace is a
duplicate. 7808 top-level forms get no node: `deftest` 2108, `def` 1072, `s/fdef` 1060,
`s/def` 843, `defmethod` 761, `extend-protocol` 220, and about 250 from macros of the
repository. The WASM file grows by 195 KB (39 KB compressed).

## Consequences
- A function that a macro of the repository declares has no node, and its calls have no
  source. This is the same gap as for Rust macros.
- A parent namespace that requires its child namespace shows no `imports` edge, because
  the parent is the container of the child. The call edges of stage 2 show the
  dependency, from the function to the child.
- The Java files of blaze are in `deps.edn` modules, not in Maven modules. Until the
  Java front end knows `deps.edn`, their packages stand beside the modules. If both
  front ends make packages in one module, the nesting rule of ADR 18 must run over the
  union. Both are a later step.
- A Leiningen or shadow-cljs project gives no nodes.
