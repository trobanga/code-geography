import { LEVELS, type GraphNode, type Level, type NodeKind } from './types';

export interface Tree {
  byId: Map<string, GraphNode>;
  children: Map<string, string[]>;
  /** Display label: the name, for packages without the dot-prefix shared by all packages. */
  label: Map<string, string>;
}

function sharedPackagePrefix(nodes: GraphNode[]): string[] {
  let prefix: string[] | null = null;
  for (const n of nodes) {
    if (n.kind !== 'package') continue;
    const segs = n.name.split('.');
    if (prefix === null) prefix = segs;
    else {
      let i = 0;
      while (i < prefix.length && i < segs.length && prefix[i] === segs[i]) i++;
      prefix = prefix.slice(0, i);
    }
  }
  return prefix ?? [];
}

export function buildTree(nodes: GraphNode[]): Tree {
  const byId = new Map<string, GraphNode>();
  const children = new Map<string, string[]>();
  const label = new Map<string, string>();
  const prefix = sharedPackagePrefix(nodes);
  for (const n of nodes) {
    const rest = n.kind === 'package' ? n.name.split('.').slice(prefix.length).join('.') : '';
    label.set(n.id, rest || n.name);
    byId.set(n.id, n);
    if (n.parent !== null) {
      const list = children.get(n.parent);
      if (list) list.push(n.id);
      else children.set(n.parent, [n.id]);
    }
  }
  return { byId, children, label };
}

const KINDS_OPEN_AT: Record<Level, NodeKind[]> = {
  modules: [],
  packages: ['module'],
  types: ['module', 'package'],
  functions: ['module', 'package', 'type'],
};

/** Expanded set that makes every node of the level's kind visible and nothing deeper. */
export function levelExpanded(tree: Tree, level: Level): Set<string> {
  const kinds = KINDS_OPEN_AT[level];
  const out = new Set<string>();
  for (const [id, n] of tree.byId) {
    if (kinds.includes(n.kind) && tree.children.has(id)) out.add(id);
  }
  return out;
}

/** Expanded set without `id` and without any of its descendants. */
export function collapse(tree: Tree, expanded: ReadonlySet<string>, id: string): Set<string> {
  const out = new Set(expanded);
  const stack = [id];
  while (stack.length > 0) {
    const cur = stack.pop()!;
    out.delete(cur);
    for (const c of tree.children.get(cur) ?? []) if (out.has(c)) stack.push(c);
  }
  return out;
}

/** Deepest level whose expanded set is contained in `expanded`, and the ids beyond it. */
export function splitByLevel(tree: Tree, expanded: ReadonlySet<string>): { level: Level; open: string[] } {
  let best: { level: Level; base: Set<string> } = { level: 'modules', base: new Set() };
  for (const level of LEVELS) {
    const base = levelExpanded(tree, level);
    if ([...base].every((id) => expanded.has(id))) best = { level, base };
  }
  return { level: best.level, open: [...expanded].filter((id) => !best.base.has(id)) };
}
