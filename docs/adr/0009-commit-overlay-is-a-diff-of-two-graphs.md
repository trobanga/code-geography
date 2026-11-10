# 9. A commit overlay is the diff of two indexed graphs in the same model

Status: accepted (2026-11-10)

## Context
The user wants to see which modules a commit touches and which connections it adds or
removes, at each zoom level.

## Decision
The CLI indexes the tree before and after the commit and builds one union graph. Each
node may carry `change`: `added`, `removed` or `modified`. Each edge may carry `change`:
`added` or `removed`. A node is `modified` when a hash of its own source text differs;
the text of nested declarations does not count for the container. The cut carries the
changes upward: each visible node gets counts of changed descendants, and each
aggregated edge gets counts of added and removed edges. The viewer shows the overlay in
the normal diagram.

## Consequences
- The overlay needs no AI and works at each level through the same cut.
- Indexing two trees costs two full runs. That is about one second each for the test
  repository; incremental indexing is not needed yet.
- Formatting changes inside a declaration do not count if only whitespace differs.
- Renames show as removal plus addition (ADR 5).
