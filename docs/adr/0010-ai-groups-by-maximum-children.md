# 10. AI groups where a container has too many children

Status: proposed (2026-11-10) — not built, not tested

## Context
The AI needs a rule that says where to add a level. A fixed number of levels does not
fit code of different sizes.

## Decision (proposed)
A container shows at most about 10 children. A larger container gets named groups from
the AI, and a group that is still too large gets subgroups. Groups are stored in a file
under version control that a human can correct, and they change incrementally, so that
the tree stays stable across commits.

## Open points
- The limit of 10 is a guess. In the test repository only two packages exceed it; most
  large containers are classes with many methods, where a group level is probably not
  useful.
- The test repository is too tidy to show if AI groups are better than the package
  tree. A larger or less tidy repository is needed before this is accepted.
