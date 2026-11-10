# 2. A container tree with four fact levels; the file is an attribute

Status: accepted (2026-11-10)

## Context
C4 fixes four levels. Real code needs a variable number: namespaces nest, types nest,
and large containers need subgroups. Files look like a natural level but repeat another
one: in Java a file is almost always one type; in Rust each file is the root of one
module, while a module can also be inline, so a module is not necessarily a file.

## Decision
The model is one tree. Its fact levels are:

1. `module` — unit of build (Maven module, Cargo crate)
2. `package` — namespace (Java package, Rust `mod`); may nest
3. `type` — class, interface, record, enum, struct, trait; may nest
4. `function` — method, constructor, free function

Group nodes (directory or AI) may be inserted between fact nodes. The file is not a
node. Each type and function carries `file` and a line `span`.

## Consequences
- The number of levels follows from the code, not from the design.
- A commit maps to nodes through `file` and `span`, or through content hashes.
- A strict tree cannot put shared code into two groups. A DAG was rejected for the
  first version because it makes layout and edge aggregation much harder.
