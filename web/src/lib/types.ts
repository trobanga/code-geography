/** `value`: a `def` in Clojure, a leaf beside the functions of a package. */
export const NODE_KINDS = ['module', 'package', 'type', 'function', 'value'] as const;
export type NodeKind = (typeof NODE_KINDS)[number];
/** `value_ref`: a reference to a value. `implements` can also join two functions (a multimethod implementation to its multimethod). */
export const EDGE_KINDS = ['extends', 'implements', 'imports', 'type_ref', 'calls', 'value_ref'] as const;
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

/** The lines of one file where the source makes an edge (ADR 0032): 1-based, ascending, each once. */
export interface FileSites {
  /** Path like `GraphNode.file`. Absent: the file of the `from` node of the edge. */
  file?: string;
  lines: number[];
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: EdgeKind;
  change?: EdgeChange;
  /** Where the source makes the edge. In an overlay: the lines of the base for a removed edge, of the head for any other. */
  sites?: FileSites[];
}

/** A file changed by the commit (ADR 0012). Paths are relative to the repository, like `GraphNode.file`. */
export interface ChangedFile {
  path: string;
  change: NodeChange;
  /** Id of the module with the longest directory prefix. */
  module: string | null;
  test: boolean;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** Present when the graph is the union of two commits (ADR 0009). */
  diff?: { base: string; head: string; files?: ChangedFile[] };
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
  /** Focus cut (ADR 0011): per container, the number of direct children folded into `rest:<id>`. */
  rest?: Record<string, number>;
}

export const LEVELS = ['modules', 'packages', 'types', 'functions'] as const;
export type Level = (typeof LEVELS)[number];
