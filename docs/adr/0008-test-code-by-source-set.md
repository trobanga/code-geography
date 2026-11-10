# 8. Test code is marked by source set and hidden by default

Status: accepted (2026-11-10)

## Context
About half of the test repository is test code. Its edges hide the architecture: main
modules seemed to depend on the test utility module.

## Decision
A declaration is test code when its path has a directory `src` followed by a directory
other than `main` (`src/test`, `src/e2e`, `src/it`). The marker is a field on the node.
The viewer hides test code unless the user switches it on.

## Consequences
- The rule is a Maven convention. Other build systems need their own rule in their front
  end.
- A module that exists only to support tests but keeps its code in `src/main` is not
  marked.
