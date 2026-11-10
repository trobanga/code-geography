export type NodeKind = 'module' | 'package' | 'type' | 'function';
export const EDGE_KINDS = ['extends', 'implements', 'imports', 'type_ref'] as const;
export type EdgeKind = (typeof EDGE_KINDS)[number];

export interface GraphNode {
  id: string;
  parent: string | null;
  kind: NodeKind;
  name: string;
  file?: string;
  span?: { start_line: number; end_line: number };
  test?: boolean;
  change?: NodeChange;
}

export type NodeChange = 'added' | 'removed' | 'modified';
export type EdgeChange = 'added' | 'removed';

export interface GraphEdge {
  from: string;
  to: string;
  kind: EdgeKind;
  change?: EdgeChange;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** Present when the graph is the union of two commits (ADR 0009). */
  diff?: { base: string; head: string };
}

export interface ViewEdge {
  from: string;
  to: string;
  count: number;
  kinds: Partial<Record<EdgeKind, number>>;
  /** Part of `count`. */
  added: number;
  removed: number;
}

export interface ChangeCount {
  added: number;
  removed: number;
  modified: number;
}

export interface View {
  nodes: string[];
  expandable: string[];
  edges: ViewEdge[];
  /** Per visible node with a marked node in its subtree (itself included). */
  changes: Record<string, ChangeCount>;
}

export const LEVELS = ['modules', 'packages', 'types', 'functions'] as const;
export type Level = (typeof LEVELS)[number];
