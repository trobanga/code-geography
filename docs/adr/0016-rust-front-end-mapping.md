# 16. The Rust front end: how Rust maps onto the graph

Status: accepted (2026-11-10)

## Context
ADR 2 names Rust `mod` as the package level and ADR 3 plans a Rust front end. Rust
differs from Java in ways that decide the mapping: a module is not a file (a file holds
the root of one mod, an inline `mod x { }` holds another); the crate root is a mod with
no name; a function belongs to an `impl` block, not to the type, and the `impl` can sit
in another mod or file than the type; names are found through `use` declarations that
can re-export; macros can generate items that no syntax tree shows. Stage 1 builds nodes
and the edges that ADR 1 allows. Call edges are stage 2.

## Decision
- **Module** (`mod:<dir>`): each `Cargo.toml` with a `[package]` table; its name is the
  package name. A `Cargo.toml` with only `[workspace]` gives no node. The text is read
  line by line (package name, dependency names); there is no TOML dependency.
- **Package** (`pkg:<dir>:<crate>::<mod path>`): each Rust `mod`, nested. The crate root
  has no node: its mods and items are children of the module. A mod is found by
  following `mod x;` from a root to `x.rs` or `x/mod.rs`, or it is inline. The file and
  the span are attributes. A `.rs` file that no `mod` reaches is not compiled: it gives
  no nodes and is listed in the report. `#[path = ".."]` is not followed and is listed.
- **Targets**: `src/lib.rs` is the root of the package, or `src/main.rs` if there is no
  lib. Otherwise `main.rs` is a package `main` below the module. `src/bin/<n>.rs`
  (also `<n>/main.rs`), `tests/<n>.rs`, `benches/<n>.rs`, `examples/<n>.rs` are roots of
  their own, as packages `bin/<n>`, `tests/<n>`, ... below the module. `build.rs` is
  ignored. Other paths in `[lib]` or `[[bin]]` are not read.
- **Type**: `struct`, `enum`, `union`, `trait`, `type` alias. Variants and fields are no
  nodes. Id `type:<crate>::<mod path>::<Name>`.
- **Function**: a free `fn` is a child of its mod; a `fn` in `impl T` or `impl Trait for
  T` is a child of `T` if `T` resolves to a type of the repository, in whatever mod or
  file the impl stands; else of the trait, if that is in the repository; else of the mod.
  A `fn` of a trait is a child of the trait. Functions in function bodies, closures and
  macro output get no node. Ids: `fn:<crate>::<mod path>::<name>` for a free function,
  `fn:<type>#<name>` for an inherent function, `fn:<type>#<Trait as written>::<name>`
  for a trait impl (so `From<A>` and `From<B>` differ), `fn:<trait>#for <Self as
  written>::<name>` and `fn:<mod>#<Trait> for <Self>::<name>` where the type is outside.
  The ids contain `::`, so they are disjoint from Java ids, except `mod:<dir>`: a
  directory with a `pom.xml` and a `Cargo.toml` is one module node.
- **Test code** (ADR 8): everything below an item with `#[cfg(test)]` (also `all(test,
  ..)`, not `not(..)`), `#[test]` functions, and the targets `tests/`, `benches/`,
  `examples/`. The mod nodes carry the marker too, so that hiding a test mod hides its
  subtree.
- **Edges**: `implements` (type to trait, for `impl Trait for T`, both in the
  repository), `extends` (trait to supertrait), `imports` (from the mod that holds the
  `use` to the item or mod that it names; a glob gives the mod), `type_ref` (from the
  innermost declaration, function, else type, else mod, to each repository type that
  it names in a signature, a field, a body, a macro argument, or as the prefix of a
  path such as `Foo::new()`; the header and the associated types of an `impl` count for
  the type of the impl). No edge between a node and itself, its ancestor or its
  descendant. No call edges.
- **Resolution**, only what the declarations prove (ADR 1): generic parameters and items
  and `use` declarations inside a function hide names; `Self` is the type of the impl;
  then the items of the current mod, its `use` declarations (renames, groups, globs of
  repository mods, re-exports followed), the paths `crate`, `self`, `super`, the name of
  another package of the repository, and the dependencies of the manifest. A path that
  starts at a dependency, `std`, `core`, `alloc`, the prelude or a primitive type is
  external and gives no edge.
- Own-text hash and span as for Java; they include the attributes above the item. The
  hash function and the rule for the directories that hold no sources moved to
  `cg-core`, so that all front ends share them.
- The report counts the paths that resolved, that left the repository, and that could
  not be followed; it lists the unreached files and the `#[path]` attributes.

## Consequences
- Macros: items that a macro generates (`macro_rules!` output, derive output, `sqlx`
  and `serde` code) are not nodes. A path in the arguments of a macro call is read as a
  run of names joined by `::`, whatever the macro does with it; such paths are not
  counted in the report.
- Files that no `mod` reaches are missing from the graph, also those behind
  `#[path]` and those behind a `#[cfg(..)]` that selects a file by platform or feature.
  The report says how many.
- Declarations behind different `#[cfg(..)]` with the same qualified name give one
  node, the first in source order; so do `impl W<u8> { fn new }` and `impl W<u16> { fn
  new }`. Edges that only the dropped one would have given are missing.
- A file that several targets include through `mod` (for example `tests/common/mod.rs`
  in each test target, or `mod x;` in `main.rs` and `lib.rs`) is one mod in each, as
  Cargo compiles it: its nodes appear once per target.
- `where Self: Trait` is not read as a supertrait. A unit struct used as a value
  (`let x = Marker;`) gives no `type_ref`; a tuple-struct constructor (`Wrapper(1)`)
  does.
- A name from a glob import of an external crate is external if nothing else binds it;
  a name that a macro brings into scope and nothing declares is unresolved.
- An impl on a type outside the repository puts its functions below the trait or the
  mod, so a type of the repository does not show its trait impls on external types.
- The `.wasm` file grows with the grammar from 1.02 MB to 2.39 MB (gzip 0.25 MB to
  0.43 MB). The export `indexJava` of ADR 15 is now `indexSources`; its result is
  `{ graph, report: { calls, rust } }`, and `cg-index` joins the front ends.
