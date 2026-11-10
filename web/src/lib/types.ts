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
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: EdgeKind;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface ViewEdge {
  from: string;
  to: string;
  count: number;
  kinds: Partial<Record<EdgeKind, number>>;
}

export interface View {
  nodes: string[];
  expandable: string[];
  edges: ViewEdge[];
}

export const LEVELS = ['modules', 'packages', 'types', 'functions'] as const;
export type Level = (typeof LEVELS)[number];
