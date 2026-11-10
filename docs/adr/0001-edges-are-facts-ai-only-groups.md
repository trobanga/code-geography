# 1. Edges are facts; AI only groups and names

Status: accepted (2026-11-10)

## Context
The tool must show connections that a developer can trust. A language model that reads
code and reports dependencies produces a plausible graph with invented and missing edges.
A previous run of such a tool on the test repository gave 12 % inferred edges and a flat
partition of 75 communities that the user could not use.

## Decision
Nodes and edges come only from a deterministic indexer. An edge exists only when the
indexer resolves both ends. AI may partition the children of one container into named
groups. AI never adds, removes or changes a node or an edge.

## Consequences
- The picture is incomplete where the indexer cannot resolve a reference, and it says so
  by absence, not by a guess.
- The AI task is small and can be checked: input is one container with its children and
  their edges, output is a partition with names.
- Quality of the edges is a property of each language front end and must be measured.
