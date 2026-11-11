# 27. Value nodes, and nodes for `defmethod` and `extend-protocol`

Status: accepted (2026-11-11)

## Context
After ADR 24 three gaps were left in the Clojure graph of blaze. 1056 references named a
`def` var, which had no node. The bodies of `defmethod` (762 references outside test
code) and of `extend-protocol` and `extend-type` (344) had no source node. blaze wires
its components with `ig/init-key` methods, so the wiring was not in the picture.

ADR 2 has four fact levels. A `def` is none of them: it is a named value. To call it a
function is false for a constant, and a reference to it is not a call.

## Decision
- **Node kind `value`** (fifth fact level, a leaf beside functions) and **edge kind
  `value_ref`** (a reference to a value, as `type_ref` is to a type). The core treats
  them as any other kind. `CallReport` gets `to_value`. Java and Rust make no value
  nodes yet; their graphs do not change.
- **Clojure `def` and `defonce`**: a value node `val:<ns>/<name>`. If the value is a
  literal `(fn ..)`, the node is a function (`fn:<ns>/<name>`). The value form is read
  as a function body, with the node as the source. A symbol that names a value gives
  `value_ref`, in call position too: the indexer cannot know that a value is callable.
  `declare` gives no node. The first top-level form that declares a name in a namespace
  gives the node; later ones are counted.
- **`defmethod`**: a function node, child of the namespace that holds the form. Name:
  the multi as written, a space, the dispatch value as written (`ig/init-key
  :blaze.db/node`). Id: `fn:<ns>/<multi> <dispatch>`, where an alias in the multi is
  replaced by the full namespace name, so that a renamed alias changes no id. The space
  keeps it apart from a `defn` id. The body is read from the dispatch value on. An
  `implements` edge goes to the `defmulti` if that is in the repository. So
  `implements` can join two functions.
- **`extend-protocol` and `extend-type`**: each (type, protocol, method) is one function
  node, by the precedent of ADR 16: a child of the type if that is a type of the
  repository, else of the protocol if that is in the repository, else of the namespace.
  Id: `ext:<type>#<protocol>/<method>`, with the full class name for an imported class.
  An `implements` edge goes from the type to the protocol if both are in the
  repository. `(extend T P {..})` with a map is not read and is counted.

## Measured on blaze
Outside test code: 539 value nodes, 7577 functions, 11 970 `calls`, 2121 `type_ref`,
1089 `value_ref` and 851 `implements` edges. All code: 761 `defmethod` nodes, 518
methods of `extend` forms (485 below a type or protocol, 33 below a namespace). 521 of
the method nodes have an `implements` edge; the others implement a multi of a library
(`ig/init-key` 183, `ig/halt-key!` 24). References in code without a source node, in
the forms that this ADR is about, fall to 1. No node id and no edge of the graph before
is gone. 12 random new edges were compared with the source, all correct. The index
needs 1.3 s.

## Consequences
- The component wiring is in the graph: an `ig/init-key` method is a function that calls
  the constructor of its component. The edge from the configuration key to the method
  is not, because Integrant makes it at run time.
- Dispatch is still not followed: a call of a multi ends at the `defmulti`, and the
  `implements` edges show which methods can answer.
- A value that holds a function (`(def f (partial g 1))`) is a value node, and a call
  of it is a `value_ref`.
- The viewer has a sixth edge switch and a fifth letter in the legend.
