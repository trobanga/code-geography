# 18. Java packages nest

Status: accepted (2026-11-10)

## Context
ADR 2 lists the four fact levels and says that `package` may nest, so containers inside
containers were allowed from the start. The Rust front end already nests packages (a
`mod` in a `mod`). Java has no formal package nesting: `a.b` and `a.b.c` are unrelated
packages, and the dots are a naming convention. The Java front end made one flat
`package` node per package, so that the depth control of the viewer, which opens package
layers one by one, had nothing to open in a Java code base.

## Decision
The convention is shown as structure. In each module:

- P is the set of packages that hold at least one type.
- A `package` node exists for each package of P, and for each prefix of a package of P,
  on `.` boundaries, that is not in P, under which at least two different next segments
  occur among the packages of P, unless it is the common prefix of all packages of the
  module. So `a.b.c.x`, `a.b.c.y` and `a.b.d.z` give `a.b.c` with the children `x` and
  `y`, and `a.b.d.z` beside it; `a` and `a.b` get no node. A single package `a.b.c` is
  one node, not a chain. The default package (empty name) is a node of its own and does
  not nest.
- The parent of a package node is the nearest package node whose name is a proper
  prefix, else the module.
- Ids do not change: `pkg:<module dir>:<full package name>`, also for the prefix nodes
  (ADR 5), so that links and commit overlays stay valid. Only `parent` and `name`
  change. The name of a top-level package is the full package name; a nested package is
  named relative to its parent (`fhir`, not `util.fhir`).
- A prefix node has no file, no span and no hash. Its `test` flag is set if all packages
  below it are test code. The `test` flag of a package of P stays unset, as before.
- A file without a type (`package-info.java`) gives no package node.

## Consequences
- The node ids of types and functions do not change. A package id does not change either;
  the packages that are prefixes (`util` when `util.fhir` and `util.auth` are the only
  packages below it) are new ids.
- `diff` needs no change: a node without a hash is never `modified`, and a prefix node
  that appears or disappears with a commit is `added` or `removed`. It appears when a
  second package starts to branch below it, so a commit that adds a package can add two
  nodes.
- The rule depends on the set of packages of the whole module, so a prefix node can
  appear or vanish when an unrelated package is added or removed.
- The common prefix of a module is never a node of its own. This keeps `care.smith.fts`
  out of each module, but a module whose packages all start at `a.b.c` shows no `a.b.c`.
- No edge between a node and its ancestors, as before: a type in `util.fhir` that uses a
  type in `util` gives a type-to-type edge, which the cut shows between the closed
  `util.fhir` and the type when `util` is open.
- A viewer that shortens package names by a prefix shared by all packages has to look at
  top-level packages only, because nested names are already relative.
