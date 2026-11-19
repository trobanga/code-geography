# 38. The viewer finds boxes by name

Status: accepted (2026-11-19)

## Context
A large graph (blaze: 11 300 nodes) has most boxes inside closed containers. The developer
who knows a name had to open containers by hand until the box showed.

## Decision
- A search field in the toolbar (`boxIndex`, `findBoxes` in `web/src/lib/boxSearch.ts`)
  lists the nodes of the whole graph, also those in closed containers. The cut stays in
  WASM; the search reads the tree only.
- Each word of the query (split on whitespace, capitals not apart, `words` of
  `menuSearch.ts`) must be in the node name. Not in the path, no fuzzy matching: equal names
  are told apart by the path that the list shows beside each match.
- Order: while test code is off, matches in test code come after all others; then a name
  equal to the whole query, then names that start with the first word, then the rest; then
  the shorter name; then path and id by code point. At most 50 matches are listed; the list
  says how many more there are.
- A match in test code shows a `test` tag while test code is hidden. Choosing it turns test
  code on, then reveals it. Skipping such matches would hide a test class that the developer
  looks for, with no hint why.
- Choosing a match calls `reveal` in `App.svelte`: it applies `revealDepth`
  (`web/src/lib/expansion.ts`) and selects the box. `revealDepth` expands every container on
  the path, and puts into `unfolded` each container on the path that would fold the next
  node: under a focus (ADR 0011) every one; without, a container that the kind fold folds
  (ADR 0019) when the next node on the path is not a package. Without that the box would sit
  in the placeholder `rest:<id>`.
- The diagram then pans to the box at the current zoom (`Diagram.reveal`), not a fit of the
  whole diagram: on a large graph a fit makes the box too small to read.

## Consequences
- The reveal of the detail panel uses the same function: it pans instead of fitting, and it
  also unfolds folded containers.
- `revealDepth` predicts in TypeScript which containers the WASM cut folds, as
  `levelUnfolded` does. Under a focus it cannot tell which children are of interest, so it
  unfolds the whole path: each container on it shows all its children, more than the focus
  would show, and these containers stay in `unfolded` (and the hash). The general fix is a
  set of nodes that `focusView` must keep visible; that is a change of the cut in
  `crates/core`, not made here.
- A match in test code turns test code on, which changes the cut and the layout.
- The search is a linear scan per keystroke over the lowercased names: on blaze 4 ms for a
  one-letter query, under 1 ms for a name. It needs no further index.
- The query is not in the hash.
