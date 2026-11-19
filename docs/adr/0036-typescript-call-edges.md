# 36. TypeScript call edges

Status: accepted (2026-11-19)

## Context
ADR 34 gave the TypeScript nodes and the `imports`, `extends` and `implements` edges. A
file shows what it imports, not which of its functions calls which. ADR 1 allows an edge
only if the source proves it; ADR 14 (Java) and ADR 17 (Rust) say what a call proves.

TypeScript differs from Java and Rust in ways that decide the rules:

- The compiler infers most types. The source writes a type at a parameter, often at a
  field, rarely at a local variable. Without a type checker the indexer knows the type of
  a receiver only where it is written.
- Functions are values. A callback, a component, a handler is passed around by name.
- A name has two meanings: a value (`const A`, `function A`, `class A`) and a type
  (`interface A`, `type A`, `class A`). `const A` and `interface A` can stand side by
  side.
- Much code stands outside every function node: the top level of a file, the callback of
  `memo(..)` or `forwardRef(..)` (no node, ADR 34), the initializer of a field.
- A React component is called by React, not by the code that writes `<A />`.

## Decision
- **Call site**: a `call_expression` (also a tagged template and a call through `?.`),
  a `new_expression`, `super(..)`, and a JSX element whose tag is a name with a capital
  letter or a member expression (`<A />`, `<ns.A>`). `import(..)` is an import (ADR 34),
  not a call. A lower-case tag (`<div>`) is no call site.
- **Source of the edge**: the innermost node around the call. A function node (a
  `function`, a `const` with a function, a method, a field with a function, ADR 34); a
  function inside it that is no node (a callback, an arrow function, a nested `function`,
  a method of an object literal, a class inside a function) counts for it, as in ADR 14
  and 17. Code in a class that is in no function node (a field initializer, a static
  block, a decorator, the `extends` expression) counts for the type. Code that is in no
  function and no type (the top level of a file, a `const` that is no function node, the
  callback of `memo(() => ..)`) counts for the package of the file (the module for the
  `index.ts` of a module directory), or for the namespace around it, as the initializer
  of a Rust `const` counts for its mod (ADR 17).
- **Scopes**: each function (a node or not) is a scope of its parameters, its type
  parameters and of every name that its body declares outside the functions in it (`let`,
  `const`, `var`, `function`, `class`, `interface`, `type`, `enum`, a `catch` or `for`
  binding, each name of a destructuring), wherever the block. A name of one block thus
  hides the name in the whole function: a missing edge, never a wrong one. A `static`
  block of a class is a scope like a function body. A class is a scope of its type
  parameters and, for a class expression, of its own name. `import x = A.b` binds `x`.
  A namespace body and the file are the outer scopes; their
  names are those of ADR 34 (the declarations, the imports), plus the names that a
  block at the top level declares; a namespace body is a scope of one file, also when
  another file of the package declares the same namespace. Two bodies of one namespace
  in one file share their names, which then prove nothing (a non-exported name of one
  body is not visible in the other). Values and types are looked up apart. The type of
  a variable is resolved where the variable is declared; a type parameter or a local
  type of a function around the use hides it too.
- **A plain name `f()`**: the innermost scope that binds `f` decides. A parameter or a
  local name: no edge (unresolved; external if its written type is outside the
  repository, `cb: Listener` of a library). A function node of the file or of a namespace around
  the call, declared by `function` or by `const`: the edge. A `let` or `var` with a
  function is no proof, it can be assigned again: unresolved. An import: the name is
  followed through the re-exports (`Files::lookup`, also a default import) to function
  nodes; a name that two `export *` of one file reach as two different declarations
  proves neither (the import edge of ADR 34 takes the first; a call takes none); an
  import of an external package is external; a name that reaches no function
  (a `const`) or a file that is not read is unresolved. A name that no scope binds is a
  global of the language or of a library (`setTimeout`, `require`): external. Nothing
  hides the name if no scope between the call and the binding binds it; the scopes are
  complete for the forms above, so this is a proof within the file.
- **`ns.f()`** for `import * as ns`: `f` is looked up in the file that `ns` reaches. For
  a namespace `N` of the file: the function `N.f`.
- **Method call `x.m()`**: the type of the receiver comes from what is written:
  - `this` in a class (a method, a field, an arrow function in them): the class. A
    `function` or an object literal method between rebinds `this`: unknown.
  - `super.m()`: the class that `extends` names (ADR 34 resolution).
  - A parameter with a type, a `const`, `let` or `var` with a type, a `const` with
    `new T()` (also at the top level of the file), a field of a class with a type or
    with `new T()`, a constructor parameter with `private`, `public`, `protected` or
    `readonly` (a field), a property of an interface with a type; `a.f.m()` through such
    fields; `new T().m()`; `x!` and `(x)` as `x`.
  - `T.m()` for a class `T` (a static call), also `ns.T.m()`.
  - A written type is a name, also with type arguments (`Store<A>` is `Store`), and a
    union with `null` and `undefined` (`T | undefined`, `x?: T`). The name is resolved as
    in an `extends` clause (ADR 34): a type parameter or a type declared inside a
    function hides it.
  - Unknown: `any` and `unknown`, an inferred type, the result of a call or of a
    getter, a type
    parameter, another union, an
    intersection, an object type, a function type, `typeof`, an `as` cast, an element
    of an array, a destructured name, an imported `const` (its type is in another file).
  - Known and external: a type of a library or of the language (`Map`, `HTMLElement`,
    `Promise`), a primitive (`string`), an array (`T[]`, `Array<T>`): external.
- **Method lookup** in a class or interface `T` of the repository: the members of `T`,
  then of its supertypes (`extends`, then `implements`), nearest first. A method, a
  method signature, an accessor or a field with a function (one node per name, ADR 34):
  the edge to that node. A call through an interface goes to the signature of the
  interface, not to an implementation; a call through an abstract method to the
  abstract method. Two supertypes at the same distance that declare the name: the edge
  to `T`. A field (a property with no function node) declares the name: the edge to the
  type that declares the field (`to_type`). No member of that name and a supertype
  outside the repository (`extends React.Component`): external. No member and all
  supertypes in the repository: a member of `Object` or `Function` (`toString`,
  `bind`): external. No member and a supertype that is a name of the repository but no
  class or interface (an unresolved import, a `type` alias, `extends mixin(A)`):
  unresolved. A receiver whose type is a `type` alias or an `enum`: unresolved, their
  members are no nodes. A name that reaches two classes (two re-exports) is unknown.
- **`new T()`** and **`<T />`** of a class: the `constructor` of `T` if it declares one,
  else the type `T` (as for an implicit constructor in ADR 14). `super(..)`: the same
  for the class that `extends` names. `new f()` of a function node: the function.
- **JSX**: `<A />` is a call site of `A`, resolved as a plain name, and `<ns.A />` as
  `ns.A`. React calls the component, not the code that writes the element; but the
  element is the only reference of a component in its parent, and the edge says what a
  reader of the code wants to know: which component renders which. The report counts
  these sites apart (`jsx_elements`).
- **A function as a value**: a name that is an argument of a call or of `new` (`map(f)`,
  `addEventListener('x', onX)`) or the value of a JSX attribute (`onClick={onSave}`), and
  that reaches a function node by the rules above (also `ns.f`, `this.m`, `x.m` with a
  known type), gives the `calls` edge, as in ADR 17 and 24. The report counts these
  (`function_values`). A name in another place (an object literal, a `return`, an
  assignment) gives no edge; a name that reaches no function node is no call site. A
  getter named there (`f(this.size)`) is called by the access itself: the edge holds,
  though it is counted as a value.
- **Edges and sites**: one `calls` edge per `(from, to)`; the site (ADR 32) is the line
  of the resolved name: the function name, the property of a member call (in a chain over
  several lines, its own line), the type name of `new`, the tag of a JSX element, the
  name of a function value. Edges between a node and itself, its ancestor or its
  descendant are dropped (ADR 34), so a method that calls a method of its own class
  keeps its edge, and a call in a field initializer to its own class has none.
- **Report**: `Report.typescript.calls` is a `cg_core::CallReport` (to function, to
  type, external, unresolved; `to_value` stays 0). `function_values` and
  `jsx_elements` count apart; both are also in the four counts. `cg-index` adds
  `calls` to `Report.calls`, so the viewer summary and `cg index` show the sum, and
  `cg index` prints a `typescript calls` line.

## Rejected
- **Types from return types** (`a.b().c()` through the written return type of a unique
  function, as in ADR 14 and 17). In TypeScript the return type is rarely written. Later.
- **Types of imported `const`s** (`export const api = new Api()`): needs the value
  declarations of the other file. Later; the report shows the share as unresolved.
- **Narrowing by `T extends B`** of a type parameter (ADR 17 does it for Rust trait
  bounds). Generics in TypeScript are mostly data, rarely bounded services.
- **A JSX element as no call**: a React code base would show no edge between its
  components; these are most of its functions.
- **A function value anywhere** (Clojure, ADR 24): an identifier in an object literal
  or an assignment is a value that can be called by anyone; the two forms taken are the
  ones whose meaning is a call by the callee.

## Consequences
- Measured with `cg index`. The viewer of this repository (`web`): 7763 call sites,
  1910 (25 %) to a function (21 as a value), 49 to a type, 2878 (37 %) external, 2926
  (38 %) unresolved; 382 `calls` edges, 139 of them from a package (the top level of
  the test files: `describe(..)`). `excalidraw` (commit `d732d4f`): 66 128 call sites,
  25 254 (38 %) to a function (340 as a value), 107 to a type, 20 035 (30 %) external,
  20 732 (31 %) unresolved; 1430 of the sites are JSX elements; 9605 `calls` edges, 7599
  from functions, 1853 from packages, 153 from types. The index of excalidraw takes
  1.8 s natively, parse included. A sample of 35 edges (25 of excalidraw, 10 of `web`) held at the line of the
  site in each case.
- A call is resolved within one file plus the imports, exports and declarations of the
  others: no type inference, no flow. No edge there is a missing edge, not a wrong one.
- The scopes are by function, not by block: a name that one block of a function binds
  hides the same name of the file in the whole function.
- Calls in code at the top level of a file give edges from its package; calls of the
  file's own functions there have no edge (descendant).
- A global function that a `.d.ts` of the repository declares (`declare global`) counts
  as external, as for `extends` (ADR 34).
