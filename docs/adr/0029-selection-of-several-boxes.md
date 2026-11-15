# 29. The viewer can select several boxes

Status: accepted (2026-11-15)

## Context
The viewer had one selected box. Two questions need more than one: "how do these boxes
depend on each other" and "open these containers", which was one double-click and one
layout run per container.

## Decision
- The selection is a list of boxes. A click selects one box. Shift+click adds a box or
  removes it; Shift+click on the background keeps the selection. The selection is not
  in the hash.
- Lines get one of three strengths (`lineStrength`). One selected box: its lines are
  full, all others dim, as before. Two or more: a line with both ends selected is full,
  a line with one end selected is weak, all others are dim.
- The edge option `between selected` (hash `between=1`) makes the lines with one end
  selected dim also. Then only the lines between the selected boxes stay.
- Only the two ends of a view line count. A line from a child of a selected open
  container is not a line of that container.
- `open all` / `close all` in the side panel (`openOrClose`): if a selected container
  is closed and can open, all such containers open. If not, each open selected
  container closes with all below it. One step, one layout, then fit.
- With two or more boxes the side panel lists the boxes, not their lines.

## Consequences
- `on selection` (O1) paints the lines at each selected box.
- `open all` does not ask above the box limit, as a double-click does not.
- More operations on a selection (focus cut on the selected boxes, fit, move together)
  are possible on the same list; `Focus.nodes` is a list already.
