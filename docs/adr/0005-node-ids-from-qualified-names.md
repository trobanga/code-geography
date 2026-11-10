# 5. Node ids come from qualified names

Status: accepted (2026-11-10)

## Context
A commit overlay compares the graphs of two revisions. That needs ids that stay the same
when unrelated code changes.

## Decision
Ids are built from names only: `mod:<dir>`, `pkg:<module dir>:<package>`,
`type:<package>.<Outer>.<Inner>`, `fn:<type>#<name>(<parameter types as written>)`.
No line numbers, no file paths (except the module directory), no counters.

## Consequences
- Moving code inside a file, or moving a file inside a package, keeps the id.
- A rename, a moved type, or a changed parameter type gives a new id. The overlay shows
  it as one removed and one added node. Rename detection is not planned yet.
