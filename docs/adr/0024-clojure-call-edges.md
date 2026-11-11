# 24. Clojure call edges

Status: accepted (2026-11-11)

## Context
ADR 23 gave the Clojure nodes and the `imports` edges. ADR 1 allows an edge only if the
source proves it, and ADR 14 and 17 say what a call edge is for Java and Rust.

Clojure has no receiver types to follow, but it has something better for this purpose: a
symbol with a namespace part (`d/type-query`) can never be a local name, and the `ns`
form says which namespace the alias means. So a qualified symbol is proved by two lines
of the same file. An unqualified symbol is harder: a local can hide a var, and a macro
can bind names in a way that no syntax tree shows.

## Decision
Stage 2a reads qualified symbols only.

- **Where**: the body of a top-level `defn`, `defn-`, `defmacro`, `defmulti` (the source
  is that function) and of a method of a `defrecord` or `deftype` (the source is that
  method).
- **Resolution**: the namespace part is an alias of the `ns` form (`:as`), else the full
  name of a namespace of the repository. Then the name is looked up in the vars of that
  namespace.
- **`calls`**: the name is a function node. A protocol method is a var of the namespace
  of its protocol, so `(p/-get store k)` goes to `fn:<ns>/Store#-get`. A function named
  as a value (`(map a/f xs)`, `#'a/f`) gives the edge too, as in ADR 17; the report
  counts these.
- **`type_ref`**: the name is a protocol or interface, or a constructor that `defrecord`
  or `deftype` generates (`->Rec`, `map->Rec`).
- **`implements`**: from a `defrecord` or `deftype` to each protocol or interface of the
  repository that its body names. An unqualified name is resolved here (same namespace,
  or `:refer`), because no local can stand at that place.
- **Not read**: a quoted form, a form under `#_`, `(comment ..)`, metadata, keywords,
  the binding vectors `{:keys [a/f]}`. A syntax-quoted form is read only in a
  `defmacro`: the expansion calls what it names.
- **No edge, counted**: an alias of a namespace outside the repository and a name such
  as `Math/abs` (external); a name that the namespace declares with `def` (var without
  node); a name that the namespace does not declare (unresolved; a macro can have made
  it); a reference in code that has no function node (`def` value, `defmethod`,
  `extend-protocol`, other top-level forms).
- No edge between a node and itself, its ancestor or its descendant.
- Dispatch is not followed. A call ends at the protocol method or the `defmulti`, as a
  Java call ends at the interface method.

## Measured on blaze
1.2 s for the index. 6212 call sites give an edge (561 as a value), 119 go to a type,
30 950 are external, 1056 name a `def` var, 170 are unresolved. Outside test code the
graph has 6525 `calls`, 1744 `type_ref` and 186 `implements` edges. The `calls` edges
join 247 of the 417 pairs of modules that `imports` joins.

What stage 2a leaves out, outside test code: 2315 references in code without a function
node (`defmethod` 762, `extend-protocol` 344, `def` 179, other 1030). In all code, 4034
unqualified call heads name a function of the same namespace and 859 name a function
that `:refer` brings; for 110 of these 4893 the name also occurs in a place where the
same top-level form could bind it.

## Consequences
- The edges between namespaces are nearly complete without scope analysis. The edges
  inside a namespace are missing until unqualified symbols are read.
- The count of 110 in 4893 shows that a simple rule is sufficient for the next stage: no
  edge if the top-level form could bind the name anywhere. It loses about 2 % of the
  sites and needs no model of the binding forms of `clojure.core`.
- 1056 references to `def` vars have no target. Whether a `def` gets a node is open.
