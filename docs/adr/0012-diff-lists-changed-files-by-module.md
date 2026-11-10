# 12. The diff graph lists all changed files and attaches each to a module

Status: accepted (2026-11-10)

## Context
The overlay of ADR 9 sees only changes inside declarations of an indexed language. For
14 of the last 15 commits of the test repository it is empty: they change `pom.xml`,
YAML or documents. Changes in a source file outside a declaration (imports, file
comments) are invisible too.

## Decision
`cg diff` records each changed file of the commit in `diff.files`: path, change
(`added`, `removed`, `modified`), the module with the longest directory prefix (or none)
and the test flag of ADR 8. Source files are in the list too. A file is not a node
(ADR 2); the viewer shows the files as a count on the module and as a list. Renames are
not followed.

## Consequences
- Each commit shows at least which modules it touches.
- A file says nothing about connections. A changed `pom.xml` can add a module
  dependency, and the overlay does not show that as an edge.
- The viewer can show a source file with a change that no node explains.
- A file of a module that the test filter hides has no box to carry its mark.
