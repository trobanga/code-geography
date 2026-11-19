# 37. Elixir call edges

Status: accepted (2026-11-19)

## Context
ADR 35 gave the Elixir nodes and the `imports` and `implements` edges. A module shows
which modules it names in a directive, not which of its functions calls which; ecto's
`lib` writes 649 calls with a full `Ecto` module name and only 175 directives. ADR 1
allows an edge only if the source proves it; ADR 14, 17, 24 and 36 say what a call
proves in Java, Rust, Clojure and TypeScript.

Elixir differs from those in ways that decide the rules:

- A remote call names its module (`Mod.fun(..)`), and the aliases of the lexical scope
  say which module the name is (ADR 35). There is no receiver type to infer.
- A function is a name and an arity. Default arguments make one definition answer
  several arities.
- A local call `fun(..)` reaches a function of the module, an imported function, or
  `Kernel`. The compiler refuses a local call that two of these can answer: a local
  function that conflicts with an import, and a call that two imports answer, are
  compile errors. A variable is never called as `fun(..)` (that is `fun.(..)`).
- `use M` runs the macro `M.__using__`. It can add aliases, imports and functions that
  no syntax tree of the module shows (ADR 35, Consequences).

## Decision
- **Call site**: a call whose target is a written name: `fun(..)` and `fun ..` (a local
  call), `Mod.fun(..)`, `Mod.fun ..` and `Mod.fun` with an alias, `__MODULE__` or an
  atom before the dot (a remote call). The right side of `|>` adds one to the arity
  (`x |> Mod.fun(y)` is `fun/2`, `x |> fun` is `fun/1`). A capture `&Mod.fun/2` or
  `&fun/1` is a function value with its written arity; `&Mod.fun(&1, 3)` is a call of
  arity 2. A `defdelegate f(a), to: M, as: :g` is a call site of `M.g/1` (`M.f/1`
  without `as:`) at the line of `M`. No call site: a bare name (`foo` is a variable),
  `f.(x)` (a function value), `map.field` (an identifier before the dot, no
  parentheses; also `a.b.c` and `@x.field`), a modifier of a bitstring segment
  (`size(8)` in `<<n::size(8)>>`), an operator, the head of a definition (`def f(a)`), a directive, a
  form in `quote` (code of the module that uses it, ADR 35), the typespec attributes
  (`@type`, `@typep`, `@opaque`, `@spec`, `@callback`, `@macrocallback`: `String.t()`
  is a type), and the attributes that belong to a clause (ADR 35).
- **Source of the edge**: the function node around the call (all clauses of a name and
  arity are one node): its body, its guards, its default values. A call in a module
  body outside every function (an attribute value, the arguments of `plug` or
  `schema`, a `test` block, a definition whose name is not written) has the package of
  the module as its source, as the top level of a TypeScript file (ADR 36). The
  condition of an `if` or `unless` in a module body is such code. A call outside every
  module (the top level of a file) has no source: it is counted apart
  (`calls_outside_module`), not in the classes.
- **Arity**: a function `f` with `n` parameters as written and `d` default arguments
  (`\\`, the largest `d` of its clauses) answers the arities `n - d` to `n`. The node
  stays `f/n` (ADR 35).
- **Remote call `Mod.fun`**: `Mod` is expanded by the aliases of the scope (ADR 35);
  `__MODULE__` is the module around the call. Then:
  - A module of the repository with a public function (`def`, `defmacro`, `defguard`,
    `defdelegate`, a protocol signature) of that name that answers the arity: the edge
    to it. A protocol function is the signature: dispatch is not followed, as in ADR 27.
  - A module of the repository with no such public function (a function that a macro
    made, `Repo.all` after `use Ecto.Repo`, a private function, `__struct__`), in a
    function: the edge to the package of the module (`to_type`). The call proves the
    module, not the function. The report lists these names (`undeclared_functions`).
    The same call in a module body outside every function stands in the argument of a
    macro, which may be a type (`field :b, M.t()` of `typedstruct`) or code that the
    macro does not run: no edge, unresolved (`macro_argument`).
  - Main code is compiled without test code: a call in main code reaches only modules
    that main code declares. A module that only test code declares (a fixture, a mock
    of `String`) is for main code a module outside the repository.
  - A module that the repository does not declare, below one that it declares
    (`MyApp.Missing`): unresolved, as for the references of ADR 35. Any other module,
    an Erlang module (`:lists.map`): external.
  - A variable before the dot (`mod.fun()`), `unquote(m).fun()`, the result of a call:
    unresolved (`dynamic`).
- **Local call `fun(..)`**, in order:
  1. The module around the call has a function (also private) of that name that
     answers the arity: the edge to it. A conflicting import would be a compile error,
     so no import and no `use` can take the call. This also holds for a function with
     the name of a `Kernel` function: the module must `import Kernel, except:` it.
  2. The `import`s of the scope (lexical, as the aliases: `import M`, `only:` and
     `except:` with a keyword list, `only: :functions` and `:macros`) of modules of the
     repository (a later `import` of a module replaces an earlier one of it): if
     exactly one public function of them answers the name and the
     arity, the edge to it. Two answers: unresolved (`ambiguous`); the code would not
     compile. An `only:` or `except:` that is not a literal list makes the import a
     maybe, and a maybe answer gives no edge (unresolved). An import of a module
     outside the repository cannot take a call that a module of the repository
     answers: that, too, is a compile error.
  3. A name of `Kernel` or `Kernel.SpecialForms` (`if`, `case`, `raise`, `to_string`,
     `apply`, `unquote`, `super`): external. `apply/2,3` thus never gives an edge.
  4. Anything else (a function of an import or `use` of a dependency, of a `use` of
     the repository, a DSL macro such as `test`, `assert`, `field`, `plug`):
     unresolved (`local_unknown`).
- **What `use` does to the proof.** Function names: nothing. A function or an import
  that a `use` adds to the module conflicts with a local function or with another
  import that answers the same call, so rules 1 and 2 hold whatever a `use` adds; a
  call that they do not answer stays without an edge. Aliases: a `use` can make an
  alias that changes what a remote call names. For each remote call the walk keeps the
  `use`s of the scope that stand after the alias that expands the first segment (all
  `use`s of the scope if no alias expands it; none for `Elixir.A`). For each such
  `use M`:
  - `M` is a module of the repository with a `defmacro __using__/1`, or with a `using`
    block in its body (`ExUnit.CaseTemplate` makes the `__using__` of that block): the
    aliases that `M` can make are those of the `alias` and `require .., as:` directives
    and of the
    `defmodule` forms in any `quote` in the body of `M` (in any function, as Phoenix
    builds them in `def controller`), and those of the modules that a `use` in such a
    `quote` names, transitively. An alias whose name is not written (`alias
    unquote(m)`, `alias __MODULE__`) can be any name.
  - `M` is a module of the repository without a written `__using__` (it gets one from
    a macro): it can make any alias.
  - `M` is outside the repository (also a `use` of it written in such a `quote`): it
    can make an alias of the last segment of a module that its arguments write (`use Lib, repo: MyApp.Repo` can `alias
    unquote(repo)`), and no other. A library knows the names of the repository only
    through its arguments; its own aliases are names of the library.
  If one of them can make an alias of the first segment, the call gives no edge
  (unresolved, `use_alias`). The report counts the edges that stand only on the third
  point (`behind_outside_use`): an edge whose first segment is not aliased in the
  source, with a `use` of a module outside the repository after it. That is one of the
  two ways to a wrong edge that are left (Consequences), and the one that is counted.
- **Directives keep ADR 35.** The `imports` edges of `alias`, `import`, `require` and
  `use` do not apply the rule above; ADR 35 states that gap.
- **Edges and sites**: one `calls` edge per `(from, to)`; the site (ADR 32) is the line
  of the function name (of the module in a `defdelegate`). No edge between a node and
  itself, its ancestor or its descendant: a recursive call has none, nor a call from
  a module body to its own functions.
- **Report**: `Report.elixir.calls` is a `cg_core::CallReport`: `to_function`,
  `to_type`, `external`, `unresolved` (`to_value` stays 0). Apart:
  `function_values` (captures, also in `to_function`), `unresolved_calls` by reason
  (`use_alias`, `ambiguous`, `dynamic`, `local_unknown`, `below_declared`,
  `macro_argument`), `behind_outside_use` (call sites), `calls_outside_module`, and
  the names of the `to_type` calls (`undeclared_functions`: `Mod.fun/n`, with how
  often). `cg-index` adds `calls` to `Report.calls`, so the
  viewer summary and `cg index` show the sum; `cg index` prints an `elixir calls` line.

## Rejected
- **Trust every `use`** (the rule of ADR 35 for directives): `Repo.all` after a `use`
  of the repository that aliases `MyApp.Repo` as `Repo` would go to a top-level module
  `Repo` of the repository. A macro of the repository knows its names; its `quote` is
  there to read.
- **Distrust every `use`**: the modules of a `use` of a library (`use GenServer`, `use
  TypedStruct`, `use ExUnit.Case`) would lose every remote call whose first segment is
  not aliased. The cost is measured below.
- **An edge to the function that a `use` defines**: it has no node (ADR 35).
- **Following `apply(M, :f, args)` with literal arguments**: rare, and `apply` is for
  names that are not known when the code is written.
- **Operators** of the repository (`a <~> b`): an operator is a function node (ADR 35),
  but a call of it needs the same lookup as a local call with no name to read the
  site from. Later.

## Measured
On 2026-11-19 with `cg index`, on `elixir-lsp/elixir-ls` at `b8a61c9` (umbrella, 1014
files) and `elixir-ecto/ecto` at `4f388df` (130 files), the same file counts as in ADR
35. The index takes 0.8 s and 0.5 s, as before this slice.

| call sites | elixir-ls | ecto |
|---|---|---|
| all | 37 077 | 23 477 |
| to function (as a value) | 9305, 25.1 % (81) | 8575, 36.5 % (78) |
| to type | 53, 0.1 % | 0 |
| external | 11 217, 30.3 % | 5287, 22.5 % |
| unresolved | 16 502, 44.5 % | 9615, 41.0 % |
| of these: local unknown, dynamic, macro argument | 15 488, 83, 930 | 8640, 195, 772 |
| of these: a use could alias, ambiguous, below a declared module | 0, 0, 1 | 0, 0, 8 |
| behind a use of a dependency (an edge or to type) | 985 | 1229 |
| outside a module (not in the classes) | 22 | 26 |
| `calls` edges, outside test code | 3498, 2988 | 2214, 1718 |
| pairs of packages that `calls` joins outside test code, of these also `imports` | 1319, 202 | 152, 18 |

Most unresolved sites are local calls of macros and functions that an import or a
`use` of a dependency brings: `test`, `assert`, `describe`, the `field` of
`typedstruct` (583 modules of elixir-ls), the query macros of `Ecto.Query`. `dynamic` is
mostly `repo.insert(..)` on a variable in ecto's tests.

The rule for `use` costs no edge on either repository. Before the `using` block counted
as a `__using__`, the two case templates of elixir-ls (`use ExUnit.CaseTemplate`) made
1075 call sites in its tests unresolved; their `using` blocks only `import`. To
distrust every `use` would cost the 985 and 1229 call sites that stand behind a `use`
of a dependency with a first segment that no alias expands. The rule for a call in a
module body turned 930 sites of elixir-ls (692 edges between packages, nearly all
`field :x, M.t()` of `typedstruct`) and 772 sites of ecto (13 edges, its tests calling
`Ecto.TestRepo.insert!`, whose functions `use Ecto.Repo` makes) into `macro_argument`.
The rule for test code removed 573 edges from main code of elixir-ls to `String`, a mock
of Elixir's `String` below `test/misc`.

The `.wasm` file grows from 7 992 966 to 8 074 250 bytes (gzip 1 154 851 to
1 176 034).

32 random edges (16 of each) were compared with the line of their site: 30 calls were
right. The other two were `field(:data, GenLSP.TypeAlias.LSPAny.t())` of `typedstruct`
read as calls to the package; the rule for module bodies came from them and removes
both.

## Consequences
- The proof of a local call rests on the compiler: the edge is right for code that
  compiles and runs the call as written.
- Two ways to a wrong edge are left:
  1. A call in the argument of a macro that rewrites it and never makes the call:
     `from p in Post, select: count(p.id)` in a module that declares or imports a
     `count/1` gives an edge to that function, though `Ecto.Query.from` turns
     `count` into SQL. The same holds for a remote call in such an argument, inside a
     function. The report has no count for these sites; the indexer cannot tell them
     from a call.
  2. An alias that a macro makes and the indexer does not see, with the name of the
     first segment of a module of the repository. For a library macro (an alias
     without a module of its arguments) the report counts the call sites that stand
     on that assumption (`behind_outside_use`). For a macro of the repository whose
     `quote` the indexer does not read it has no count: a `__using__` that takes its
     `quote` from a function of another module, or a `defmacro` other than
     `__using__` whose `quote` holds `alias`.
- Calls in a function to functions that a macro defines in a module of the repository
  go to the package of that module; the same calls in a `test` block give none.
- No edge from code at the top level of a file.
- Test code still reaches a mock of a module of Elixir that test code declares: the
  tests of elixir-ls call its mock `String` (most of the 53 sites to type, and
  `String.length/1`).
- In a quote, a `defmodule` or `use` whose name is not written, and `alias __MODULE__`,
  count as an alias of any name. A guard (`defguard`) is a macro for `only:
  :functions` and `only: :macros`. A `defdelegate` with default arguments is a call
  site of its written arity.
