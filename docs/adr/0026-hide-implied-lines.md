# 26. The viewer can hide implied lines

Status: accepted (2026-11-11)

## Context
The module level of a large repository cannot be read as a node-link picture. blaze has
73 modules and 417 lines between them; ELK puts the lines into shared channels, and no
single line can be followed. The edge kind switches do not help, because most pairs of
modules are joined by `imports` alone. The cause is structural: a layered code base
repeats each dependency. If A uses B and B uses C, A mostly uses C too. The module
graph of blaze has no cycle, and its transitive reduction has 118 lines.

Rejected: a threshold on the count of a line (the cut is arbitrary, and a dependency
with one use is a dependency); "on selection" as the default (it shows no structure
without a click). A dependency matrix is the complete answer for a dense graph and
stays possible as a second view.

## Decision
- A switch `hide implied` in the edge row. A line A→C is implied if the drawn lines
  hold a path from A to C through another box.
- The rule runs on the lines that the view would draw (after the cut, lift and the kind
  switches), as a flat graph of their end points. It takes the strongly connected
  components. A line between two components is hidden if the condensation has a path
  between them through a third component. A line inside a component is never hidden,
  because the reduction of a cycle is not unique.
- Never hidden, but used for paths: a changed line of a commit overlay, and a line with
  an `extends` or `implements` part.
- The layout gets the reduced set. A hidden line of the selected box is drawn as a
  dashed straight line, without a new layout. The side panel lists all dependencies of
  the selected box, as before.
- The switch is `auto` until the user clicks it: on if the view would draw more than
  100 lines, else off. A click makes the choice explicit and puts it into the URL hash
  (`implied=1`, `implied=0`). The line counter says how many lines are hidden.
- The logic is a pure function in the viewer (`web/src/lib/implied.ts`). It is not a cut
  and changes no fact of the graph.

## Measured on blaze
Module level: 119 lines drawn of 417, 298 hidden; the layout needs 80 ms in place of
570 ms. Packages level: 574 drawn, 370 hidden. The fit scale does not change (0.51):
the size of the picture comes from the boxes and layers, not from the lines.

## Consequences
- The picture shows the layers and hides direct dependencies that a path repeats. A
  reader who wants to know "does A use C directly" must select A or switch it off.
- At the fit scale the labels stay about 5 px, and ELK still merges lines into trunks
  (`mergeEdges`), so a single line is followed with selection and zoom, not by eye.
- Where the drawn graph has large cycles, the switch hides little.
