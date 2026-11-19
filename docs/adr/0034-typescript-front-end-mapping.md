# 34. The TypeScript front end: how TypeScript maps onto the graph

Status: accepted (2026-11-19)

## Context
The fourth language is TypeScript. A TypeScript repository gave an empty picture. This
ADR is stage 1: nodes and the edges that need no call resolution. Call edges are stage 2
and get an ADR of their own.

TypeScript differs from Java, Rust and Clojure in ways that decide the mapping:

- A file is a module, and it has no name but its path. An import names a file by a path
  (`./forge`, `../lib/forge.js`), not by a name that the file declares. ADR 5 wants ids
  from qualified names and no file paths; here the path is the qualified name.
- There is no unit of build in the language. `package.json` is the nearest thing: a
  repository can hold many (workspaces), they nest, and a file belongs to the nearest one.
- A file can pass on what it imports (`export { A } from './a'`, `export * from './a'`).
  A directory is imported through its `index.ts`.
- Most imports of an application do not start with `./`. In `excalidraw`, 1000 of the
  3700 imports of `packages/excalidraw` name another package of the repository
  (`@excalidraw/element`). In an application with the alias `@/*` nearly all do. Whether
  such a name is a file of the repository or a package of `node_modules` is decided by
  `tsconfig.json` (`paths`, `baseUrl`) and by the `package.json` files of the repository.
- Functions are values: `const f = () => {}` is the usual form of a function.
- Tests have no source set. The test runners find them by name.
- The grammar crate (`tree-sitter-typescript` 0.23.2) has two languages, one for `.ts`
  and one for `.tsx`, each with parse tables of the size of the Rust grammar.

## Decision
- **Input**: `.ts` and `.tsx` (so also `.d.ts`), `package.json`, `tsconfig.json` and
  `tsconfig.*.json`. Not `.mts`, `.cts`, `.js`, `.jsx`, `.vue`, `.svelte`. A `.ts` file
  is parsed with the TypeScript language, a `.tsx` file with the TSX language. A `.ts`
  file that starts with `<` or holds a NUL character is not TypeScript (a Qt translation,
  a video stream): it is not read, and the report counts it.
- **Module** (`mod:<dir>`): each directory with a `package.json` that is the nearest
  `package.json` above at least one file that is read. Its name is the `name` of the
  `package.json`, else the last segment of the directory, else the root name. Modules do
  not nest: a file belongs to the nearest `package.json` above it. A `package.json`
  without such a file (the root of a workspace, a package of plain JavaScript) gives no
  node, so a repository of another language with a `package.json` for its tools keeps its
  graph. A file with no `package.json` above it is not read; the report counts these.
- **Package** (`pkg:<dir>:/<path>`): a file and a directory. `<path>` is the path
  relative to the module directory, for a file without `.ts` or `.tsx`. The file
  `a/index.ts` is the directory `a`, as an import reads it, so `a.ts`, `a.tsx`,
  `a/index.ts` and the directory `a` are one package `/a`. The `index.ts` of the module
  directory has no package: its declarations are children of the module (as the root of
  a Rust crate, ADR 16). Each file that is read is a package, also one without
  declarations: its imports need a source. Directories nest by the rule of ADR 18 with
  `/` as the separator: a directory is a node if a file gives it (`index.ts`, or `a.ts`
  beside `a/`), or if at least two different next segments occur below it and it is not
  the common directory of all packages of the module. The name of a package is relative
  to its parent package; a top-level package has its path in the module (`src/lib`).
  A package has no file and no span, as in the other front ends.
- **Namespace**: `namespace N { }` (also `module N { }`, `declare namespace`) is a
  package below the package of its file, `pkg:<dir>:/<path>#N`; `namespace A.B` gives `A`
  and `A.B`. Its declarations are nodes. `declare module "name" { }` and
  `declare global { }` declare things outside the file: they are not read.
- **Type** (`type:<dir>:/<path>#<Name>`): `class`, `abstract class`, `interface`, `type`
  alias, `enum`, at the top level of a file or of a namespace, exported or not, also
  behind `declare`. In a namespace the name is `N.Name`. Enum members and fields are no
  nodes.
- **Function** (`fn:<dir>:/<path>#<name>`): a `function` declaration (also `async`,
  generator, an overload signature, `declare function`), and a top-level `const`, `let`
  or `var` whose value is an arrow function or a function expression. Below a type
  (`fn:<dir>:/<path>#<Type>.<name>`): the methods of a class (also `constructor`,
  accessors, static and abstract methods, overload signatures), a field of a class whose
  value is an arrow function or a function expression, and the method signatures of an
  interface. A property of an interface with a function type is a field, not a method.
  A name that is computed (`[Symbol.iterator]`) gives no node.
- **Default export**: `export default class Foo` and `export default function foo` are
  nodes by their name. Without a name (`export default function () {}`, a class, an
  arrow function) the name is `default`, and a method of such a class is `#default.m`.
- **No node**: a `const` that is not a function (also `memo(() => ..)`, an object, a
  call of `styled` or `defineComponent`), a declaration inside a function, a class
  expression, a method of an object literal. There are no value nodes (ADR 27) yet.
- **One id, one node**: the first declaration gives the node, in path order of the files
  and source order in a file; a declaration with a body replaces an overload signature
  of the same id. So the overloads of a function, a getter and its setter, and the two
  parts of a merged `interface` are one node, with the span of the declaration that gave
  it.
- **Ids**: every id below the module holds `<dir>:/`, the module directory and a path
  that starts with `/`. No id of the Java, Rust or Clojure front end holds that, so the
  ids are disjoint from theirs; `mod:<dir>` is shared on purpose, as before. The module
  directory is in the type and function ids, because a path like `/src/index` occurs in
  every package of a repository. A file that moves gets new ids, as a Java class that
  changes its package. A new `package.json` between a file and its old one changes the
  ids of the files below it.
- **Test code** (ADR 8): a file whose name is `test.ts`, `spec.ts` or ends with
  `.test.ts` or `.spec.ts` (the same for `.tsx`), and each file below a directory
  `__tests__` or `__mocks__`. These are the defaults of Jest and Vitest. The nodes of
  such a file and its package carry the marker; a directory carries it if all packages
  below it do. The changed-file list of an overlay uses this rule for `.ts` and `.tsx`
  files, not the rule of ADR 8, which calls `src/lib/x.ts` test code.
- **Edges**:
  - `imports`, from the package of the file to what an `import` or an `export .. from`
    names. A name (`import { A }`, `import A`, `export { A } from`) goes to the type or
    function that it reaches, through the re-exports of the files between, also through
    `export *` and through a name that a file imports and exports again. A name that
    reaches no node (a `const`, a namespace, a name of an external package) goes to the
    package of the file that the import names. `import * as x`, `import './x'`,
    `export * from` and `import('./x')` with a string go to that package. `import type`
    gives the same edge: the dependency is in the source. `require` is not read.
  - `extends`, from a class to the class and from an interface to each type that its
    `extends` clause names; `implements`, from a class to each type of its `implements`
    clause. The name is resolved in the namespaces around the declaration, then in the
    file, then through the imports of the file (`A`, or `x.A` for `import * as x`).
    A name that nothing in the file declares or imports is a global of the language or of
    a library: no edge. An expression (`extends mixin(A)`) and a name that the file
    declares as something else than a type (`const Base = mixin()`) give no edge.
  - No `type_ref` and no `calls` edges in this stage. No edge between a node and itself,
    its ancestor or its descendant.
  - Sites (ADR 32): the line of the name in the import or in the clause; for an edge to
    a package or a module the line of the string that names the file.
- **Resolution of an import**, only what the files prove (ADR 1):
  - A relative path (`./x`, `../x`) is joined to the directory of the file. `x.ts` and
    `x.tsx` name that file. `x.js` stands for `x.ts`, `x.tsx` or `x.d.ts`, `x.jsx` for
    `x.tsx`. Else `x.ts`, `x.tsx`, `x.d.ts`, `x/index.ts`, `x/index.tsx`,
    `x/index.d.ts`, the first that is read. The file can be in another module.
  - `paths` and `baseUrl`: the `tsconfig.json` of the nearest directory above the file
    that has one, and the files that its `references` name, each with what it gets
    through a relative `extends`. In each of them the pattern with the longest prefix
    that matches the name is taken, and its targets in their order (relative to
    `baseUrl`, else to the file that holds `paths`); without a pattern, the name below
    `baseUrl`. The import resolves if every configuration that has a pattern for the
    name gives the same file. A pattern that matches and finds no file leaves the name
    to the packages of the repository, as the compiler goes on to `node_modules`; if
    none has the name, the import is unresolved. The pattern `*` claims no name that it
    does not find. `tsconfig` files are JSON with comments and trailing commas; a byte
    order mark at the start of a configuration file is skipped.
  - The name of a package of the repository: exactly one `package.json` has it as
    `name`. Then the import goes to the file that the `package.json` proves: for the
    package itself the `exports` entry `.`, else `types`, `typings`, `module` and
    `main`, else `index`; for a path below it (`@a/b/x/y`) the `exports` entries that
    match (also with one `*`), or the path in the package directory if there is no
    `exports`. An entry counts if the rules for a relative path find a file that is
    read, and all entries that count must give the same file. If the `package.json`
    proves no single file (it names `dist/index.js`, or two entries give two files),
    the edge goes to the module node; if the package has no module node (no file of it
    is read), the import counts as unresolved. A name that two `package.json` files
    have is unresolved: which of them the import means is not in the files.
  - Everything else leaves the repository (`react`, `node:fs`): no edge.
- **Report**: the imports that reach the repository, that are external, that should
  reach the repository and do not (a relative path without a file, an alias that no
  `tsconfig` resolves: `@/x`, `~/x`, `#x`, a package of the repository without a
  module), and the relative imports of files of another kind (`./a.css`, `./A.svelte`).
  The names of the resolved imports that reach a node and those that reach only the
  package. The names of `extends` and `implements` clauses that reach a type, that are
  external, and that reach none. The files that are not read (no `package.json`, not
  TypeScript) and the files that the grammar parses with an error.
- Own-text hash and span as for Rust: from the `export` or the first decorator to the
  end; the hash of a type leaves out its methods. One of several declarators of a
  `const` has the span of the declarator.
- `package.json` and `tsconfig` are read with `serde_json`, which the workspace has.

## Rejected
- **A module for the files outside every `package.json`** (the root). A Qt repository
  has `.ts` files that are XML, and a repository without `package.json` has no unit of
  build to show. A Deno project stays empty until `deno.json` counts as a module.
- **Every directory is a package node.** Nearly every module would open to a single box
  `src`. ADR 18 solved the same for Java.
- **One grammar for both kinds of file.** The TypeScript language does not parse JSX.
  The TSX language does not parse the type assertion `<T>x`, which is valid in a `.ts`
  file: it reads a JSX element, and the declarations after it in the file are lost or
  land inside the function. Of 2868 `.ts` files (nest, excalidraw, the viewer, the
  TypeScript of blaze and of the Rust test repository) one has such an assertion, so the
  second language is bought for few files; code of before 2019 has more. It costs
  1.45 MB of the WASM file (139 KB compressed). It is in, because the loss is silent and
  takes the rest of a file; with the TSX language alone the report would count such a
  file as a syntax error and nothing more.
- **Test code by a directory `test` or `tests`.** It would hide the helpers of tests,
  and also the main code of an application whose domain has tests. The marker is hidden
  by default, so a wrong marker costs more than a missing one.
- **The source directory behind `dist`.** A `package.json` that names `dist/index.js`
  means `src/index.ts` in most repositories; `outDir` and `rootDir` of a `tsconfig`
  could prove it. Not read: the edge goes to the module.

## Consequences
- The two languages of the grammar add 3.2 MB to the `.wasm` file (4,754,188 to
  7,907,667 bytes; with gzip 785 KB to 1113 KB). The file is the largest part of the
  first load of the viewer.
- Measured with `cg index` at this stage. The viewer of this repository (`web`, 1 module):
  74 packages, 125 types, 308 functions, 378 `imports` and 4 `extends` edges; of 190
  imports 144 resolve, 43 are external, 3 name files of another kind, none is unresolved.
  `excalidraw` (commit `d732d4f`, 12 modules, 700 files): 745 packages, 829 types, 3095
  functions, 7694 `imports`, 8 `extends` and 6 `implements` edges; of 5218 imports 4335
  resolve, 561 are external, 322 name files of another kind (`.scss`, `.json`), none is
  unresolved; 6759 imported names reach a declaration, 2110 only the package (a `const`);
  5 files have a syntax error.
- The grammar is of 2024. It does not know `export type * from` (TypeScript 5.0) and
  `typeof import('x')` as a type argument, and it fails on a decorator before an
  overload signature. The tree then has an error, the report counts the file, and the
  nodes around the error are still read. `export type * from` is read as `export *`
  (the tree keeps the source), and `typeof import('x')` as an import of `x`.
- A function that is the argument of a call (`memo(function A() {})`,
  `forwardRef(..)`) has no node. In a React code base these are many components.
- An import of a `const` shows the file, not the value. The report says how many names
  end at a package.
- A global declaration of the repository (a `.d.ts` without imports, `declare global`)
  is not found by the name in an `extends` clause; such a name counts as external.
- The `index.ts` of a module directory is the module: an import in it of a file of the
  module is an edge to a descendant and is dropped.
- `a.ts` beside `a/index.ts`, or `a.ts` beside `a.tsx`, are one package. If both declare
  the same name, the first file in path order gives the node.
- Of the parts of a merged declaration (two `interface A`, a class and an interface of
  one name) only the part that gives the node is read for `extends` and `implements`.
- A local directory with built files (`dist/index.d.ts`) is read with them: they are
  packages of the module, and a `package.json` that names `dist/index.js` then proves
  that file. A forge does not hold them.
- A relative import whose last segment has a dot and that finds no file
  (`./app.service`) counts as a file of another kind, not as unresolved.
- A class and a namespace of the same name in one file share ids below them
  (`#A.b` is the method `b` of the class and the function `b` of the namespace): one
  node.
- The name of a package of the repository is taken as that package, also if the
  installed version would come from the registry (a version range that the workspace
  does not satisfy). The Rust front end reads the name of a crate the same way.
- A `tsconfig` that a tool writes at build time (`.nuxt/tsconfig.json`,
  `.svelte-kit/tsconfig.json`) is not in the repository: its aliases (`$lib`, `~/`) do
  not resolve and are counted. An alias of a bundler configuration (`resolve.alias` in
  `vite.config.ts`) is not read.
- `extends` of a `tsconfig` that names a package (`@tsconfig/svelte`) ends the chain.
  `include`, `exclude` and `files` are not read: the nearest `tsconfig.json` counts for
  a file, as an editor takes it.
- The viewer shortens the names of top-level packages by a prefix on `.`, not on `/`:
  the names stay as they are (`src/lib`).
- `.vue` and `.svelte` files hold TypeScript and are not read: in such an application
  only the `.ts` files beside the components are in the picture.
