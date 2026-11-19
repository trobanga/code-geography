# 35. The Elixir front end: how Elixir maps onto the graph

Status: accepted (2026-11-19)

## Context
The fourth language is Elixir. It differs from the other three in ways that decide the
mapping:

- The grammar (`tree-sitter-elixir`) gives calls, aliases (`A.B.C` is one node),
  operators and blocks. `defmodule`, `def` and `alias` are calls like any other: what a
  call means is the work of the front end.
- The build unit is a Mix project, a directory with a `mix.exs`. An umbrella project is
  a root `mix.exs` with `apps_path:` and one project per directory below `apps/`.
  `mix.exs` is code: the source directories (`elixirc_paths`) are often the result of a
  function call.
- A module is named by its `defmodule`, not by its path. A file can hold several
  modules, and modules nest lexically: `defmodule Inner` in `A.B` is `A.B.Inner`. `A.B`
  can be a module with functions and also the prefix of `A.B.C`, as a Clojure namespace.
- There are no types besides the module. A struct is its module (`defstruct`), a
  protocol is a module (`defprotocol`), a behaviour is a module with `@callback`
  attributes, and `defimpl P, for: T` makes the module `P.T`.
- A function is identified by name and arity. It is written as one or more clauses.
- A reference to a module is exact once the aliases of the lexical scope are known.
  There is no import path to search. What is not exact: `use` runs a macro of the named
  module, which can add aliases, imports and functions that no syntax tree shows.

This is slice 1: nodes, and the edges that need no call resolution. Call edges are
slice 2 and get an ADR of their own.

## Decision
- **Input**: `.ex` files, `mix.exs`, and `.exs` files below a directory `test`. Not
  below a directory `deps` or `_build` (the dependencies and the build output of Mix),
  nor below one that `cg_core::holds_no_sources`. Other `.exs` files are scripts and are
  no input: `config/*.exs`, migrations and seeds below `priv/`, `.formatter.exs`.
  Templates (`.eex`, `.heex`) are no input.
- **Module** (`mod:<dir>`): each directory with a `mix.exs`. Its name is the atom of the
  first `app:` in the `mix.exs`, else the last segment of the directory, else the root
  name. A `mix.exs` with `apps_path:` is the root of an umbrella and gives no module,
  as a `Cargo.toml` with only `[workspace]`. A source file belongs to the nearest
  `mix.exs` above it; if there is none, or it is an umbrella root, the file is not read
  and the report counts it.
- **Source directories**: `lib`, or the string literals that the `mix.exs` holds in the
  list of `elixirc_paths:` or in the body of a function `elixirc_paths` (all clauses: the
  indexer does not know the Mix environment). And `test`. A `.ex` file is read if it is
  below one of these directories of its project, a `.exs` file if it is below `test`.
  `mix.exs` itself gives no nodes. The report counts the input files that are not read.
- **Test code** (ADR 8): a file below the directory `test` of its project, or of a
  `mix.exs` further up. So a project below `test` (a fixture) is test code as a whole,
  and its module node carries the marker. The package nodes carry the marker by the rule
  of ADR 23. The changed-file list of an overlay uses the same rule
  (`Layout::test_code`).
- **Package** (`pkg:<dir>:<Module.Name>`): each `defmodule`, `defprotocol` and
  `defimpl`, by its full name. At the top level of a file the full name is the name as
  the aliases above it expand it. Inside a module it is the name of that module, a dot,
  and the name as written; the first segment becomes an alias for the rest of the outer
  module, as in Elixir. `Elixir.A` is `A` everywhere. The packages nest by the rule of
  ADR 18, per project, over the full names: `A.B.C` without a module `A` or `A.B` gets
  the prefix nodes of that rule and no others. A package has no file, span or hash.
- **`defimpl P, for: T`** is the module `P.T` (the full names of both), and one module
  per entry if `for:` is a list. Without `for:` the type is the enclosing module. By the
  precedent of ADR 16 and 27 the package is a child of the package of `T`, named by the
  protocol (`Jason.Encoder` in `MyApp.User`), if `T` is a module of the same project.
  Else it nests by its name, which puts it below the protocol if that is in the project.
  A module below such a `P.T` by its name (a `defmodule` in the `defimpl`) stands in it.
- **Function** (`fn:<Module.Name>.<name>/<arity>`, shown as `<name>/<arity>`): `def`,
  `defp`, `defmacro`, `defmacrop`, `defguard`, `defguardp` and `defdelegate`, children
  of their module. The signatures of a `defprotocol` are functions too. The arity is the
  number of parameters as written: `def f(a, b \\ 1)` is `f/2` only. All clauses of a
  name and arity are one node. Its hash is the text of all clauses. Its span runs from
  the first clause to the last one that follows it with no clause of another function
  between. The `@doc`, `@spec`, `@impl` and `@deprecated` attributes directly above a
  clause belong to the clause. An operator is a function by its symbol (`<>/2`).
- **Where a definition is read**: at the top level of a file (modules) and directly in
  the body of a module, and there also in the blocks of an `if` or `unless` (code that is
  compiled only if a dependency is loaded). Not in a function body, in `quote`, or in
  the block of any other macro.
- **No node**: `defstruct`, `defexception`, module attributes (`@type`, `@callback`),
  a definition whose name is not written (`def unquote(name)(..)`, `defmodule
  unquote(m)`, `defmodule :atom`), and what a macro declares (`schema`, `plug`, `test`).
  The report counts the calls directly in a module body that give no node, by their
  head (`defstruct`, `schema`, `def` for a name that is not written; `other` for a call
  of a function of another module). It does not count attributes and directives.
- **Duplicate module**: if two forms declare the same full name, the first one gives
  the nodes: main code before test code (so that a fixture never takes the name of a
  module of main code), then in the order of the paths, then of the source. The other
  form is skipped with all that is in it, and the report lists its place. The ids of
  functions hold no project directory, as the type ids of Java, so a module name is
  unique in the repository.
- **Aliases** (the lexical scope): `alias A.B` (as `B`), `alias A.B, as: C`,
  `alias A.{B, C.D}`, `require A.B, as: C`, `__MODULE__`, `__MODULE__.X`, and the alias
  that a nested `defmodule` makes. An alias holds from its line to the end of the block
  that it stands in (the module body, the function body, a `do` block, a clause of
  `fn`). A nested module starts with the aliases of the place where it stands. The name
  of an alias is expanded by the aliases above it.
- **Edges**. `imports`: from the module that holds an `alias`, `import`, `require` or
  `use`, in its body or in one of its functions, to the module that it names.
  `implements`: from the module that holds `@behaviour B` to `B`, and for `defimpl P,
  for: T` from `T` to `P`. An edge exists only if both ends are modules that the
  repository declares (ADR 1). The site (ADR 32) is the line of the name; in
  `alias A.{B, C}` the line of `B`; for a `defimpl` the line of the protocol. No edge
  between a node and itself, its ancestor or its descendant. A directive in `quote` is
  not a directive of the module that holds the `quote`: no edge, and no alias.
- **Report**: per kind of reference (`alias`, `import`, `require`, `use`, `behaviour`,
  `defimpl`, `for`) how many resolved, were external, unresolved or dynamic. A
  reference is unresolved if the repository does not declare the module, but declares a
  module above it (`MyApp.Missing` beside a module `MyApp`): such a name is not one of a
  dependency. The report lists these names. A reference is dynamic if it is not a
  written name (`import unquote(m)`). An Erlang module (`:lists`) is external.
- **Ids**. `fn:A.B.f/2` ends in `/` and digits, and a Clojure function name is never a
  number; Java function ids hold `#` and `(`, Rust ids `::`. `mod:<dir>` is shared on
  purpose. `pkg:<dir>:<Name>` is the scheme of Java and Clojure: an Elixir module and a
  Java package of the same name in one directory would be one node. Each segment of an
  Elixir module name starts with an upper-case letter, so this needs a Java package
  that is named like an Elixir module.
- Own-text hash as for the other front ends (`cg_core::text_hash`).
- The crate has the shape of `cg-index-rust`: `mix` (projects), `syntax`, `scope` (the
  aliases and the expansion of a written name), `declare` (the one walk of a file: it
  keeps the aliases of each block, so slice 2 reads the calls in this walk), `edges`.

## Measured
On 2026-11-19, on the default branch of two public repositories, with `cg index`.

| | `elixir-lsp/elixir-ls` (umbrella) | `elixir-ecto/ecto` (one project) |
|---|---|---|
| `.ex` and `.exs` files, lines | 1014, 121 396 | 130, 71 761 |
| time | 0.8 s | 0.5 s |
| `mix.exs` files | 30 | 2 |
| modules outside test code | 3, one per app | 2 (`ecto`, `examples/friends`) |
| packages, functions outside test code | 819, 2301 | 106, 1226 |
| `imports`, `implements` edges outside test code | 238, 2 | 39, 6 |
| all nodes, all edges | 4051, 515 | 1817, 273 |
| references resolved, external, unresolved, dynamic | 586, 1606, 1, 0 | 362, 92, 0, 0 |
| files outside the source directories, duplicate modules | 7, 9 | 0, 0 |
| forms without a node | 1585 (`test` 687, `typedstruct` 583) | 1191 (`test` 884, `schema` 113) |

The 22 fixture projects of elixir-ls are below `test` and are test code; before that
rule they stood as 22 modules beside the three apps. The edges that join two apps
outside test code: 23 from `language_server` and 4 from `debug_adapter`, all to
`elixir_ls_utils`. 26 random edges (14 and 12) were compared with the line of their
site, all correct. The one unresolved name is an alias of a prefix that is no module.
A first rule for "unresolved" (a module of the repository with the same first segment)
gave 244 and 12: names such as `ElixirSense.Core.State` (a dependency) and `Mix.Task`,
because the repositories declare `ElixirSense.Core.References.Tracer` and
`Mix.Tasks.Ecto`. Of the external references of elixir-ls, 684 are a `use` and 664 an
`import`. ecto has few edges outside test code: its `lib` has 175 lines with a
directive and 649 calls that write an `Ecto` module name in full. These dependencies
need the call edges of slice 2.

The `.wasm` file grows from 3 136 874 to 4 754 188 bytes (gzip 607 238 to 795 153).
No grammar before added as much (Rust: 1.37 MB, ADR 16).

## Consequences
- ADR 1 holds for the source as written, not for what its macros expand to, as in the
  Rust and the Clojure front end. An alias that a `use` adds is not known. If a
  reference starts with such an alias, and the repository declares a module with the
  name as written, the edge goes to that module. This is the one way to an edge that is
  not proven. It needs a macro that makes an alias with the name of the first segment
  of a top-level module of the repository, and the report cannot count it. Slice 2
  meets the same aliases at the calls (`Repo.all` after `use MyApp.DataCase`) and has
  to decide there whether to read the `quote` of a `__using__` of the repository.
- What a `use` adds to a module is missing: functions, imports, the `@behaviour`. The
  `imports` edge to the module of the `use` is there.
- A directive is an edge of its module, also when it stands in one function. The call
  edges of slice 2 start at the function.
- A parent module that names its child (`alias __MODULE__.Child`, `alias MyApp.Web`
  in `MyApp`) shows no edge, as for Clojure.
- The body of a module has no hash: a commit that changes only a `defstruct`, a `use`
  or an attribute marks no node as modified.
- Tests declare no nodes besides the module: `test` and `describe` are macros. A module
  that a test defines in its body is not read.
- A module in `priv/` (a migration), in `config/` or in `mix.exs` is not in the graph.
- A `defimpl` for a type of another project, or for a type outside the repository, does
  not stand in its type. If its protocol is outside too (`defimpl Jason.Encoder, for:
  Decimal`), it is a package of its own name without an `implements` edge.
- A project that sets `elixirc_paths` by an expression that holds no string literal is
  read as if it had `lib`.
