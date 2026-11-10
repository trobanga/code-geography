# 3. One tree-sitter front end for each language

Status: accepted (2026-11-10)

## Context
Exact indexes (SCIP, language servers) need a full build for each commit. That makes a
commit view slow and makes an indexer in the browser impossible. Tree-sitter parses a
commit in milliseconds and compiles to WASM, but gives syntax only.

## Decision
Each language gets a front end made of the tree-sitter grammar and hand-written name
resolution rules. The tool supports only languages with a tree-sitter grammar. Java is
first, Rust is second.

## Consequences
- Type edges resolve well through imports, packages and nesting.
- Call edges resolve only when the receiver type is written in the source. Chained calls
  and inferred types fail. Expect missing edges at the function level.
- Precision and recall are unknown until one comparison against an exact index
  (for example `scip-java`) is done. This is open work.
- Each new language costs a front end; everything else is shared.
