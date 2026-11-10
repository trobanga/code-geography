import ELK, { type ElkNode } from 'elkjs/lib/elk-api';
import workerUrl from 'elkjs/lib/elk-worker.min.js?url';
import { edgeIndex } from './elkGraph';

export interface Point {
  x: number;
  y: number;
}
export interface LaidNode {
  id: string;
  /** Id of the enclosing visible node, null at top level. */
  parent: string | null;
  x: number;
  y: number;
  w: number;
  h: number;
  container: boolean;
  depth: number;
}
export interface LaidEdge {
  /** Index into view.edges. */
  index: number;
  points: Point[];
}
export interface Layout {
  nodes: LaidNode[];
  edges: LaidEdge[];
  width: number;
  height: number;
}

let elk: InstanceType<typeof ELK> | null = null;
let workerError: Promise<never> | null = null;

function getElk(): InstanceType<typeof ELK> {
  elk ??= new ELK({
    workerUrl,
    workerFactory: (url?: string) => {
      const w = new Worker(url!);
      workerError = new Promise((_, reject) => {
        w.addEventListener('error', (e) => reject(new Error(`layout worker: ${e.message || 'failed to start'}`)));
      });
      return w;
    },
  });
  return elk;
}

/** Runs ELK in a web worker so the page stays responsive. Coordinates are absolute (json.*Coords = ROOT). */
export async function runLayout(graph: ElkNode): Promise<Layout> {
  const e = getElk();
  const res = await Promise.race([e.layout(graph), workerError!]);
  const nodes: LaidNode[] = [];
  const walk = (n: ElkNode, depth: number) => {
    const parent = depth === 0 ? null : n.id;
    for (const c of n.children ?? []) {
      const container = (c.children?.length ?? 0) > 0;
      nodes.push({ id: c.id, parent, x: c.x ?? 0, y: c.y ?? 0, w: c.width ?? 0, h: c.height ?? 0, container, depth });
      walk(c, depth + 1);
    }
  };
  walk(res, 0);
  const edges: LaidEdge[] = [];
  const collect = (n: ElkNode) => {
    for (const e of n.edges ?? []) {
      const points: Point[] = [];
      for (const s of e.sections ?? []) {
        points.push(s.startPoint, ...(s.bendPoints ?? []), s.endPoint);
      }
      if (points.length > 1) edges.push({ index: edgeIndex(e.id), points });
    }
    for (const c of n.children ?? []) collect(c);
  };
  collect(res);
  return { nodes, edges, width: res.width ?? 0, height: res.height ?? 0 };
}
