import { borderLine, type Rect } from './drag';
import type { Point } from './layout';

/**
 * Strongly connected components of a directed graph on `0..n-1` (Tarjan, iterative), numbered
 * in reverse topological order: every edge between two components goes to a lower number.
 */
function components(n: number, succ: number[][]): Int32Array {
  const comp = new Int32Array(n).fill(-1);
  const index = new Int32Array(n).fill(-1);
  const low = new Int32Array(n);
  const onStack = new Uint8Array(n);
  const stack: number[] = [];
  let next = 0;
  let count = 0;
  for (let root = 0; root < n; root++) {
    if (index[root] >= 0) continue;
    const work: [number, number][] = [[root, 0]];
    index[root] = low[root] = next++;
    stack.push(root);
    onStack[root] = 1;
    while (work.length > 0) {
      const top = work[work.length - 1];
      const v = top[0];
      if (top[1] < succ[v].length) {
        const w = succ[v][top[1]++];
        if (index[w] < 0) {
          index[w] = low[w] = next++;
          stack.push(w);
          onStack[w] = 1;
          work.push([w, 0]);
        } else if (onStack[w]) low[v] = Math.min(low[v], index[w]);
        continue;
      }
      work.pop();
      if (work.length > 0) {
        const u = work[work.length - 1][0];
        low[u] = Math.min(low[u], low[v]);
      }
      if (low[v] === index[v]) {
        let w: number;
        do {
          w = stack.pop()!;
          onStack[w] = 0;
          comp[w] = count;
        } while (w !== v);
        count++;
      }
    }
  }
  return comp;
}

/**
 * The lines that a path shows anyway: u→v with u and v in different strongly connected
 * components, where the condensation has a path from the component of u to that of v through
 * at least one other component. A line inside a component is never implied (in a cycle the
 * reduction is not unique). Parallel lines count as one edge. A protected line is never in the
 * result but counts for the paths. Returns indices into `lines`.
 */
export function impliedLines<T extends { from: string; to: string }>(lines: readonly T[], isProtected: (e: T) => boolean): Set<number> {
  const ids = new Map<string, number>();
  const id = (s: string) => {
    let i = ids.get(s);
    if (i === undefined) ids.set(s, (i = ids.size));
    return i;
  };
  const ends = lines.map((e) => [id(e.from), id(e.to)] as const);
  const n = ids.size;
  const succ: number[][] = Array.from({ length: n }, () => []);
  for (const [a, b] of ends) succ[a].push(b);
  const comp = components(n, succ);
  const m = comp.reduce((a, c) => Math.max(a, c + 1), 0);
  // Condensation, without loops and without parallel edges.
  const csucc: Set<number>[] = Array.from({ length: m }, () => new Set());
  for (const [a, b] of ends) if (comp[a] !== comp[b]) csucc[comp[a]].add(comp[b]);
  // reach[c]: components reachable from c by a path of length ≥ 1; via[c]: by a path of length ≥ 2.
  const words = (m + 31) >>> 5;
  const reach = new Uint32Array(m * words);
  const via = new Uint32Array(m * words);
  // Components are numbered in reverse topological order: successors come first.
  for (let c = 0; c < m; c++) {
    for (const s of csucc[c]) {
      reach[c * words + (s >>> 5)] |= 1 << (s & 31);
      for (let k = 0; k < words; k++) {
        reach[c * words + k] |= reach[s * words + k];
        via[c * words + k] |= reach[s * words + k];
      }
    }
  }
  const hidden = new Set<number>();
  ends.forEach(([a, b], i) => {
    const ca = comp[a];
    const cb = comp[b];
    if (ca === cb || isProtected(lines[i])) return;
    if (via[ca * words + (cb >>> 5)] & (1 << (cb & 31))) hidden.add(i);
  });
  return hidden;
}

/**
 * The hidden lines in and out of the selected boxes, as straight segments from border to border
 * between the boxes as they are; no new layout.
 */
export function impliedSegments<T extends { from: string; to: string }>(
  lines: readonly T[],
  selected: readonly string[],
  rects: ReadonlyMap<string, Rect>,
): { line: T; points: Point[] }[] {
  return lines.flatMap((line) => {
    const a = rects.get(line.from);
    const b = rects.get(line.to);
    if ((!selected.includes(line.from) && !selected.includes(line.to)) || !a || !b) return [];
    return [{ line, points: borderLine(a, b) }];
  });
}
