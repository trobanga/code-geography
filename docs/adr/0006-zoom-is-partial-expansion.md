# 6. Zoom is partial expansion of containers

Status: accepted (2026-11-10)

## Context
A global level ("everything at type level") shows hundreds of boxes: 185 types or 892
functions in the small test repository. The user wants to open one module down to its
functions while the rest stays closed.

## Decision
The state of the view is a set of expanded containers. `Graph::cut` shows a node when
each of its ancestors is expanded, moves each edge to the nearest visible ancestors of
its ends, counts the merged edges, and drops edges inside one closed container. Global
level buttons are only shortcuts that fill the set.

## Consequences
- An edge between a function in an open module and a closed module is one line with a
  count.
- An edge between a node and its own ancestor is not drawn (the layout engine handles it
  badly); it is listed in the detail panel.
