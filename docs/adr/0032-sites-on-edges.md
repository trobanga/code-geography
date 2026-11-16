# 32. An edge keeps the lines of its references

Status: accepted (2026-11-16)

## Context
The source view (ADR 31) shows the declarations on the two ends of a line, but not where
the one uses the other. The front ends saw the place of each reference and dropped it,
because an edge is unique per `(from, to, kind)`: ten calls are one edge.

Three facts decide what a place is:
- The reference need not stand in the lines of its `from` node. A Java import stands
  above the type. A Rust `impl` block stands outside its `struct`.
- It need not stand in the file of its `from` node. A Rust `impl` block or a Clojure
  `extend-protocol` can be in another file than the type that gets the edge.
- The `from` node can have no file at all: a Rust `mod` or a Clojure namespace is a
  package, and the root `mod` of a crate with `lib.rs` and `main.rs` has two files.

## Decision
- A site is one line of one file where a reference stands that the indexer resolved
  (ADR 1). An edge has `sites`: a list of entries `{file, lines}`, one per file. The
  lines are 1-based, ascending, each once: two calls on one line are one site.
- An entry leaves `file` out when it is the file of the `from` node of the edge. This is
  the usual case and keeps the graph small. An entry with a file is listed after the one
  without, in the order of the paths. An edge without sites has no `sites` field.
- The line is the line of the name that was resolved, not the first line of the
  expression around it: the method name of a call in a chain over several lines, the
  type name of a `new`, the path of an import, the symbol of a Clojure reference, the
  libspec of a `:require`.
- Sites are data like `span`. They are in no id (ADR 5) and in no identity. `Edge` no
  longer has an order or a hash: `cg_core::EdgeSet` collects the edges of a front end by
  `(from, to, kind)` and unites the sites of the same edge. The join of `cg-index` goes
  through the same type. `Edge` equality compares the sites too, so a test that compares
  graphs sees them.
- An edge is not moved to make its sites fit a span. A call in a lambda or in an
  anonymous class is a site of the edge of the function around it, as before (ADR 14).
- `diff` keeps the identity `(from, to, kind)` (ADR 9). A removed edge has the sites of
  the base, every other edge the sites of the head. A call that moved is no changed
  edge. For a removed edge whose `from` node is in another file at the head, the entry
  names the file of the base, because the node of the overlay is the node of the head.
- The viewer gives each shown line its number on both sides of an overlay (`head`,
  `base`). `withRefs` puts a site of a removed edge on the line with that base number
  and every other site on the line with that head number, so a number of one side never
  marks a line of the other.
- In the view of two boxes, an open `from` declaration marks the lines of its edges to
  the other box, and the first marked line is scrolled into view. Sites that the lines
  of the declaration do not hold are shown below them as single lines of their file,
  per file (`readDeclaration`): an import above a type, an `impl` block, the `ns` form
  of a namespace. A package with sites can be opened for these lines (`openable`). This
  changes ADR 31, where a `from` node without a span had no lines.
- For these lines the reader gives a whole file (`WHOLE_FILE`), under an overlay with
  the removed lines at their place, so that the site of a removed edge finds its line.
  A file that only the base has is read from the base: a removed edge can have its
  site in a file that the commit deleted.
- A file that the base lacks under its path is new only for a node that the commit
  added: then all its lines are marked as added. For any other node the file moved (the
  ids hold no path, so the node is not added), or it is a new file of a node that was
  there. The graph does not hold the path at the base and the reader does not guess it:
  such lines have no marks of the commit and no number at the base. Before, the reader
  marked them all as added.
- The view of one box marks nothing: there is no other end.

## Consequences
- The graph grows. Measured on 2026-11-16 with `cg index`, compact JSON:

  | repository | edges | site lines | `graph.json` before | after | growth | gzip growth |
  |---|---|---|---|---|---|---|
  | blaze (Clojure, Java) | 24 081 | 27 986 | 6 436 266 B | 7 468 038 B | +16.0 % | +15.3 % |
  | fts-next (Java) | 5 412 | 6 094 | 1 734 763 B | 1 873 759 B | +8.0 % | +10.4 % |
  | momenthafen api (Rust) | 3 837 | 4 560 | 941 927 B | 1 066 423 B | +13.2 % | +15.8 % |

  Each edge of the three graphs has a site, 1.16 lines per edge on blaze. Each edge of
  blaze has one entry, and 5 898 of these 24 081 entries name their file, nearly all for
  the `imports` edges of the namespaces; they cost about a third of the growth. The
  memory of the WASM module with the graph of blaze in an `Explorer` (four variants of
  the graph: with and without test code, ADR 8, with and without the imports that a use
  covers, ADR 13) grows from 49.0 MiB to 58.8 MiB (+20 %).
- The graphs without the `sites` fields are equal to the graphs of before, for the
  three repositories and for the overlays of 15 commits of them: no edge was added,
  dropped or marked differently.
- The plausibility of the lines was checked against the sources: on blaze the line of
  a site holds the name of the target for 99.4 % of the `calls` lines (fts-next 89.0 %,
  api 100 %) and for all `imports`, `type_ref`, `value_ref` and `extends` lines. The
  rest are right lines without that name: `this(..)` and `super(..)`, a call that only
  proves the type of its receiver (the edge goes to the type, the line holds the method),
  and `extend-protocol`, whose site is the line of the type, not of the protocol.
- A kept edge shows the references of the head only. If a commit removes one of two
  calls of the same function, the edge stays, and the removed line carries the mark of
  the commit but no mark of a reference.
- For a declaration whose file moved, the source view shows the lines of the head
  without the marks of the commit. The site of a removed edge of it is shown in the old
  file, which `diff` names. A new file of a node that was there (a Rust `impl` in a new
  file) shows no marks either, though its lines are new.
- `merge imports` drops an import that a use covers, with its sites: the line of the
  import is then not marked, the lines of the uses are.
- Calls that a front end does not resolve have no site (the known gaps of ADR 14, 17
  and 24). A marked declaration can hold further uses of the target that are not marked.
- A line with two references shows both in its tooltip; the place inside the line is
  not kept.
- A local `graph.json` carries the sites, but has no texts to show them on (ADR 31).
