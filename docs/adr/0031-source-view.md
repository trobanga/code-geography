# 31. The viewer shows the source code of a box and of a line

Status: accepted (2026-11-16)

## Context
The picture shows that two parts of the code are connected, but not the code that
connects them. The developer had to leave the viewer to read it. A node has `file` and
`span`. The texts of a forge repository are in the cache by blob sha (ADR 20); a local
`graph.json` has no texts. A line of the picture is an aggregate: between two containers
it can stand for hundreds of edges.

## Decision
- The source view is a panel below the diagram (`SourcePanel`). It opens with one or two
  selected boxes and can be hidden. It is not in the hash, as the selection is not.
- One box with a span (type, function, value): its lines, with line numbers and the file
  path. One box without a span (module, package): the list of its declarations, which
  are the topmost nodes with a span below it (`declarations`). One declaration is open
  at a time.
- Two boxes: the edges behind their line come from the graph in WASM,
  `Graph::edges_between(from, to)` and `Explorer.edgesBetween`: each edge that starts at
  `from` or below it and ends at `to` or below it. For two boxes that a cut shows, these
  are exactly the edges that the cut moves to them or below them, so the method takes
  no expanded set. This is the line as lift draws it (ADR 13), also when lift is off.
  One box inside the other gives no edges, as the cut draws no line there (ADR 6).
- The viewer groups these edges by their `from` declaration (`lineGroups`) and shows
  each declaration with the names of its targets and the kinds of the edges, once per
  direction. Edges of a kind that is switched off are left out, like in the picture.
  One declaration per direction is open at a time.
- Test code follows the `tests` option: the edges come from the variant of the graph
  that the view is cut from, and the list of declarations leaves out test nodes.
- A list shows 100 rows and gets 100 more on request. A text is read only for the
  declaration that is open.
- The texts come from `sourceReader`: the file list of the commit (the cached tree),
  then the text by blob sha from the cache, else from the forge through `Forge.text`,
  so with the token of that host only (ADR 22). The reader asks for the `Forge` at each
  read, so a token that was forgotten is not sent again. No text is kept outside the
  cache. A file with the same blob in both commits of an overlay is read once.
- With an overlay the view marks the lines that the commit added and shows the lines
  that it removed at their place. The marks come from a line diff of the file between
  the two commits (`lineDiff`, Myers, in TypeScript), not from the graph. A removed
  node takes its lines from the base commit. A removed line directly above the first
  line of a span is shown only if that first line is an added one (a replaced first
  line); else it belongs to what stood before the declaration. Update: a file that the
  base lacks under its path gives marks only for a node that the commit added, see
  ADR 32.
- A shown line is an object (`SourceLine`: number, text, mark). The place of a reference
  is not found by text search. It comes with the sites on edges (CG-3) as one more field
  of the line. Update: done, see ADR 32 (`refs`, and the numbers `head` and `base`).
- The diagram keeps the box selected last in view when its viewport changes size, by a
  pan only (`keepInView`), so the panel does not cover the box that opened it.

## Consequences
- A local graph, a local directory and `/commit.json` show the lists, but no lines, and
  a message that the source is not available.
- The source view costs no API call for a repository that is open: all inputs of the
  indexer are in the cache. If the cache lost a text, one call fetches it again.
- Under an active focus, lift drops the lines of placeholders (ADR 13). The list still
  holds their edges, so it can be longer than the count on the line says.
- A `from` node without a span (a Clojure namespace with an `imports` edge) is listed
  with its targets and has no lines. Update: it opens for the lines of its sites, see
  ADR 32.
- The marks follow the alignment of the line diff: moved code is removed lines and
  added lines. A rename shows the old declaration with its changed lines only. Above
  1000 changed lines in one file, the part between the first and the last change counts
  as replaced.
- No syntax highlighting. A span is rendered whole, also when it has thousands of lines.
