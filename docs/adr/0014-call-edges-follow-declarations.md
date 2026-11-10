# 14. Call edges follow the declarations in the repository

Status: accepted (2026-11-10)

## Context
Without call edges the graph does not show the connections between functions. A syntax
tree alone does not tell which method a call reaches. ADR 1 forbids guessed edges.

## Decision
- A new edge kind `calls`. Its source is the function whose body holds the call; a lambda
  or an anonymous class counts for the enclosing function, a field initializer or an
  initializer block for the type.
- The target is the one function of the receiver's type (or of its supertypes in the
  repository, nearest first) that fits by name and number of arguments. If several
  overloads fit, or none is declared (record accessors, Lombok, implicit constructors),
  the target is the type of the receiver.
- No edge when the receiver's type is known and not in the repository, or not known at
  all. The receiver forms that are resolved: `new T()`, `T.m()`, `m()`, `this.m()`,
  `super.m()`, `x.m()` for variables and fields, `a.f.m()`, `a.b().c()` through the
  written return type of a unique function, method references, `this(...)`, `super(...)`.
- Edges to the node itself and from a function to its own type are dropped, as for
  `type_ref`.
- The indexer reports how many call sites it resolved to a function, to a type, found
  external, or left unresolved; `cg index` prints the four numbers.

## Consequences
- Overloads with the same number of arguments are not told apart: the edge goes to the
  type, which is true but coarse.
- A call that reaches a method declared only in an external supertype can still get an
  edge to the receiver's type.
- The `var` rule, lambda parameters and chains through external types leave most call
  sites unresolved. The report makes the share visible.
