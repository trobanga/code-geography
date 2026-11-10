# 11. A focus cut folds the nodes that are not of interest

Status: accepted (2026-11-10)

## Context
The cut of ADR 6 shows all children of an open container. "Open changed" on a commit
with 18 nodes of interest gave 105 boxes and 159 lines on the test repository, because
each opened container showed all its unchanged children too. The same problem occurs for
"show this type and all that it touches".

## Decision
The cut takes an optional focus. The set of interest is the union of: explicit nodes;
the marked nodes and the ends of marked edges (if `changes`); the nodes one edge away
from that set (if `neighbors`). In an open container, a child stays if its subtree holds
a node of interest or if it is below an explicit node. All other children fold into one
placeholder `rest:<container id>` with a count. An edge of a folded node moves to the
placeholder. A container without a node of interest, or one in `unfolded`, shows all
children. An empty focus gives the plain cut. Visibility is still decided by the set of
expanded containers only.

## Consequences
- On the test commit the picture goes from 105 boxes and 159 lines to 40 and 55.
- `neighbors` pays off only while the containers of the neighbors stay closed. With
  their ancestors expanded the picture is larger than the plain cut of the changes.
- The placeholder is a view node without a graph node. The viewer must handle an id
  that is not in the graph.
- A visible ancestor keeps its true change counts; a placeholder has none.
- The lines of a placeholder are a summary of unrelated nodes. They can look like
  one dependency. The viewer must draw them as background.
