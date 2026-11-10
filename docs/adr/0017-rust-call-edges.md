# 17. Rust call edges, and one directory walk

Status: accepted (2026-11-10)

## Context
ADR 14 gave Java call edges that follow the declarations; ADR 16 left Rust without them.
Rust has no overloads, but a method call needs the type of the receiver, `impl` blocks
stand anywhere, and much of the code is generic, async or macro-driven. The directory
walk had two copies (`cg-index-java`, `cg-index`).

## Decision
- Edge `calls` from the function whose body holds the call. A closure or a nested `fn`
  counts for the enclosing function; the initializer of a `const` or `static` counts for
  the enclosing mod, or for the type of the `impl` or trait. The target is a function if
  exactly one candidate fits, else the type of the receiver; none when the receiver is
  external or unknown. Edges to the node itself, an ancestor or a descendant are dropped
  (so no edge leaves the root mod of a crate: its node is the module).
- Forms read: `f()`, `m::f()`, `T::f()`, `Self::f()`, `self.m()`, `x.m()`, `a.f.m()`,
  chains, and a function named as a value (`.map(Self::helper)`: a `calls` edge too).
  `T(x)`, `E::V(x)` and struct literals are no calls.
- Function of a type: inherent impls first, then trait impls in the repository, then
  default methods of the repository traits that the type implements. For a trait: its own
  functions and those of its supertraits. Several candidates (two `From` impls) give
  the type.
- The type of an expression comes from what is written or unique: a parameter, a `let`
  with a type or with `T { }`, `T::f()` or `f()` whose written return type is known
  (`Self` is the impl type), a field of a repository struct. The innermost `let` wins;
  a binding of `for`, `if let`, `match` or a closure parameter without a type is unknown.
  Type aliases are followed with their arguments substituted.
- Seen through: `&`, `Box`/`Arc`/`Rc` (a method is looked up in the inner type only if it
  declares it, else external), `.await` (`async fn`, `impl Future<Output = T>`), `?` on
  `Option` and `Result`, and `unwrap`, `expect`, `unwrap_or_default`. All other methods
  of `Option`/`Result`/`Future` are external and end the chain. `x.clone()` keeps the
  type of `x`. `Arc::new(e)` and `Box::new(e)` keep the type of `e`. A literal or an `as`
  cast has a known external type.
- Receivers typed by traits: `dyn Tr`, `impl Tr`, `T: Tr` (list or `where`), also inside
  `Box`/`Arc`: the function declared in the trait or a supertrait. If the trait declares
  no such function, the call is external: it can come from a supertrait outside.
- A function that a repository type does not declare: external if its name is one that
  `std` traits or derives provide (`clone`, `to_string`, `into`, `from`, `default`, `eq`,
  `cmp`, `fmt`, `as_ref`, `serialize`, ... 31 names, `STD_PROVIDED` in `calls.rs`), else
  the type (a derive or macro may have generated it).
- Arguments of macros are token trees: not read, no edge. The report counts the names
  followed by `(` in them (`macro_call_sites`).
- `Report.rust.calls` has the four counts as for Java, plus `function_values` (calls
  to a function that came from a function named as a value) and `macro_call_sites`;
  `Report.calls` of `cg-index` is the sum over the front ends. `CallReport` moved to
  `cg-core`.
- The walk is one function, `cg_walk::read_sources(root, is_input)`, in the new crate
  `cg-walk` (`cg-core` stays free of I/O). `cg-index-java` and `cg-index` call it. It
  reads every wanted file, a `pom.xml` too, and replaces invalid UTF-8.

## Consequences
- On a 178-file async Rust service (axum, sqlx) about a quarter of the call sites resolve
  to a function; most of the rest are chains whose head is an external call
  (`sqlx::query(..).bind(..).execute(..)`), `let` with an inferred type, and calls in
  macro arguments. No edge there is a missing edge, not a wrong one.
- A trait method called through a generic or a trait object gets an edge to the trait's
  declaration, not to the impl that runs; with several implementors the picture shows
  the trait, which is true.
- A function named as a value gives an edge even if it is never called through it.
- Type edges are rare: a method that a type does not declare is mostly a `std` name.
- `Option`/`Result` helpers that keep the inner type (`map_err`, `ok_or`, `ok`,
  `unwrap_or`) end the chain, though they could keep it; this is the largest known
  loss in async code.
- Two traits with a method of the same name on one type give `Several`, hence an edge to
  the type; a method that only a blanket impl in the repository provides is not found.
