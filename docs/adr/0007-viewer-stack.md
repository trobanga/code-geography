# 7. Viewer: Svelte, Tailwind, ELK, plain SVG

Status: accepted (2026-11-10)

## Context
The page loads one graph file and calls WASM. There are no routes and no server state.
The hard part is the layout of nested boxes with routed edges.

## Decision
Svelte 5 with Vite and Tailwind 4. No SvelteKit and no TanStack: nothing to route, no
server state to cache. Layout by ELK (`elkjs`, layered, hierarchy included, direction
left to right) in a web worker. Rendering as plain SVG without a graph library.

## Consequences
- Full control over boxes, edges and interaction; pan, zoom and drag are our own code.
- Layout takes about 0.3 s for packages and 2 s for all functions of the test repository.
- A full new layout after each expansion moves boxes; stable layout is open work.
