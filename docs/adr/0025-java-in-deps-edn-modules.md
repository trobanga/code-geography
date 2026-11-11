# 25. Java in `deps.edn` modules and Java interop from Clojure

Status: accepted (2026-11-11)

## Context
A Clojure repository can hold Java sources. blaze has 114 Java files in `java`
directories of seven `deps.edn` modules and no `pom.xml`. The Java front end knew only
Maven, so these files got no module: their packages stood at the top level, beside the
73 modules, in a second box with the name of the root module. A Java package and a
Clojure namespace of one module often have the same name (`blaze.fhir.spec.type`), and
the Clojure code names the Java classes through `:import`.

## Decision
- **Module of a Java file**: the nearest directory above it that has a `pom.xml` or a
  `deps.edn`. `cg-index` reads the `deps.edn` directories with the Clojure front end and
  gives them to the Java front end (`index_sources_in`); the Java crate does not know
  the Clojure grammar. A repository without `deps.edn` gives the same graph as before.
- **Test code**: a Java file in a `deps.edn` module follows the source directories of
  that `deps.edn` (ADR 23); a file outside all of them is read and is main code. If a
  directory has both files, `pom.xml` wins and the rule of ADR 8 stays, because the
  `deps.edn` rule would call `src/test/java` main code. The changed-file list of an
  overlay uses the same answer.
- **One package tree per module**: a Java package and a namespace with the same name in
  one module are one package node (`pkg:<dir>:<name>`). `cg-index` runs the nesting of
  ADR 18 again over the union of the names of both languages, for each module that has
  Java types and a `deps.edn`. Only `parent`, `name` and `test` of package nodes can
  change, no id. A package name of both languages is test code only if both are.
- **Class names**: the Java front end returns its types by binary name (`Outer$Inner`).
  The Clojure front end adds the classes of `defrecord`, `deftype`, `definterface` and
  `defprotocol` (`x.y-z/Rec` is `x.y_z.Rec`). It works without the Java table too.
- **`imports`**: from a namespace to each type of the repository that its `:import`
  clause names, also if the code does not use the class.
- **`type_ref`**: from a function to an imported or fully named class of the repository
  in `(Foo. ..)`, `(new Foo ..)`, `(Foo/method ..)`, `Foo/CONSTANT` and `Foo` as a plain
  symbol. The binding rule of ADR 24 applies to the plain symbol. A static method call
  goes to the type, not to the method node. A type hint (`^Foo x`) gives no edge.
- **`implements`**: from a `defrecord` or `deftype` to a class of the repository that
  its body names. Only an interface can stand there in valid code.
- `cg-index` drops a Clojure edge between a node and its ancestor on the joined tree.

## Measured on blaze
The top level is 73 modules. 8 package nodes hold Java and Clojure. Outside test code
the step adds 104 `imports`, 193 `type_ref` and 6 `implements` edges; 252 class
references reach a type of the repository and 1645 are external. No edge of the graph
before the step is gone. The graphs of the Java and the Rust test repository are
identical byte for byte.

## Consequences
- A type id holds no module. The same class name in two modules is one node. This was
  true before; Java in every `deps.edn` module makes it more probable.
- An `(import ..)` form outside the `ns` form is not read.
- The name munging knows `-` and a few more characters, not all.
