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
Stage 2a reads qualified symbols. Stage 2b reads unqualified symbols with a rule that
needs no model of the binding forms.

- **Where**: the body of a top-level `defn`, `defn-`, `defmacro`, `defmulti` (the source
  is that function) and of a method of a `defrecord` or `deftype` (the source is that
  method). The arities of one method, written as separate forms, are one function node.
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
- **Unqualified symbols** (stage 2b): a name of the same namespace, or a name that the
  `ns` form brings with `:refer`, `:refer :all` or `:use` (with `:only`, `:exclude`,
  `:rename`; private vars are not brought), gives the same edges as a qualified one. A
  name that two of these sources declare gives no edge (ambiguous).
- **The binding rule**: no edge if the same top-level form could bind the name, that is,
  if the name occurs anywhere in that form as an element of a vector, as a symbol of a
  map inside a vector, after `:keys`, `:syms`, `:strs`, `:as` or as a key of `:or`, as
  the name of `as->`, of `catch`, of `(fn name ..)` or of a `letfn` function. The rule
  does not know scopes: a name that is bound in one branch of the form hides the var in
  all of it. A hidden name in call position is counted; in another position it is taken
  as the local and not counted.
- The head of a method form in `reify`, `proxy`, `extend-type` and `extend-protocol` is
  a method name, not a call.
- An unqualified symbol that nothing declares (a core function, a local, interop) is no
  site. A name that only `declare` or `def` introduces is a var without node.
- No edge between a node and itself, its ancestor or its descendant.
- Dispatch is not followed. A call ends at the protocol method or the `defmulti`, as a
  Java call ends at the interface method.

## Measured on blaze
Stage 2a alone: 6212 call sites give an edge (561 as a value), 119 go to a type; the
`calls` edges join 247 of the 417 pairs of modules that `imports` joins. Outside test
code 2315 references stand in code without a function node: `defmethod` 762,
`extend-protocol` 344, `def` 179, `s/fdef` 773, `s/def` 51, macros of the repository 97,
and 109 in the second arity of a record method (corrected in stage 2b).

With stage 2b: 1.2 s for the index. 11 380 call sites give an edge (1074 as a value),
177 go to a type, 30 950 are external, 1056 name a `def` var, 396 unqualified call
heads are hidden by the binding rule (7 % of the unqualified heads that name a var of
the repository), 170 are unresolved. Outside test code the graph has 10 228 `calls`
edges (about 5200 of them inside one namespace), 1788 `type_ref` and 186 `implements`.
30 random edges were compared with the source by hand, all correct. The first run of
stage 2b had false edges from method heads in `reify`; a rule and a test now cover it.

## Consequences
- The binding rule can make a wrong edge in one case: a macro binds a name without a
  vector or a map, and a var of the namespace has that name. No such case was seen. It
  loses true edges where a function name is also a local name in the same form, and
  where a function value stands in a data vector.
- A `case` constant that names a function would be read as a reference. Not seen.
- The `ns` clause `:import` is not read: a record or a Java class that a namespace
  imports by its class name gives no edge.
- 1056 references to `def` vars have no target, and the bodies of `defmethod` and
  `extend-protocol` have no source. Both are open.
