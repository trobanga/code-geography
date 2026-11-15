# 28. Changes together: an optional layout that puts the changed boxes together

Status: accepted (2026-11-15)

## Context
`changes only` paints the changed lines and dims the boxes without a change. The layout
stays the one of all lines, so in a large repository the changed boxes are far from each
other. On the packages level of blaze (1145 boxes, 5 commits) the 63 changed leaf boxes
span 11883×7985 of a picture of 18111×9160.

A first version gave the layout the changed lines only, for each use of `changes only`.
Two problems. The boxes move when the switch changes, and the reader loses the known
places of the boxes. And if one line crosses a container border, ELK lays out the whole
hierarchy as one graph and puts all children without a line into the first layer: one
long column per container.

## Decision
- `changes only` keeps the layout. A second switch `together` (hash `together=1`, in
  effect only with `changes only`) asks for the other layout. Default: off.
- With `together`, the layout gets the changed lines only (`edgePipeline`).
- A box is active if `changes only` does not dim it, or if it is an end of a changed
  line; the containers of an active box are active (`activeBoxes`).
- In each container with active and other children, `toElkGraph` puts the active
  children into a box `layout:act:<id>` and the others into `layout:idle:<id>`. If the
  hierarchy is one graph, the active children without a line go into
  `layout:loose:<id>`. These boxes are laid out on their own, so ELK packs them.
- A `layout:` box exists in the ELK input only. `toLayout` drops it; the picture, the
  graph and the cut do not know it.

## Consequences
- blaze, packages level, lift on: 18111×9160 → 10491×7072; the changed leaf boxes span
  7397×6020. Lift off: 46775×19067 → 8072×14290, no column.
- The gain has a limit: an active container keeps its idle children, and they are most
  of its area. The focus cut on changes (ADR 0011, `open changed`) removes them and
  gives the smaller picture.
- In this layout the place of an idle box says nothing about its dependencies.
- The directions of the containers come from the estimate without the `layout:` boxes.
