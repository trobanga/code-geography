// Opening a project directory in the browser (ADR 0015): which files to read, and what to show.

/** Splits a `webkitRelativePath` into the name of the chosen directory and the path below it, with `/`. */
export function splitRelative(relative: string): { root: string; path: string } {
  const parts = relative.split(/[\\/]/);
  return { root: parts[0], path: parts.slice(1).join('/') };
}

export interface Chosen<T> {
  /** Name of the chosen directory; '' when there are no files. */
  root: string;
  inputs: { path: string; file: T }[];
}

/** The files that `isInput` wants, with their paths below the chosen directory. The rest is never read. */
export function chooseInputs<T>(files: Iterable<T>, relativeOf: (f: T) => string, isInput: (path: string) => boolean): Chosen<T> {
  let root = '';
  const inputs: { path: string; file: T }[] = [];
  for (const file of files) {
    const s = splitRelative(relativeOf(file));
    root ||= s.root;
    if (isInput(s.path)) inputs.push({ path: s.path, file });
  }
  return { root, inputs };
}

/** The call report of `indexSources`: the Java part of its report. */
export interface CallReport {
  to_function: number;
  to_type: number;
  external: number;
  unresolved: number;
}

export interface Summary {
  name: string;
  sources: number;
  nodes: number;
  edges: number;
  /** Calls to a function or a type of the graph. */
  resolved: number;
  unresolved: number;
  text: string;
  tooltip: string;
}

export function summarize(
  name: string,
  paths: string[],
  graph: { nodes: unknown[]; edges: unknown[] },
  report: { calls: CallReport },
  origin = 'Indexed in this browser; not stored. A reload returns to graph.json.',
): Summary {
  const c = report.calls;
  const sources = paths.filter(isSourceFile).length;
  const resolved = c.to_function + c.to_type;
  const text =
    `${name}: ${sources} source files, ${graph.nodes.length} nodes, ${graph.edges.length} edges, ` +
    `calls: ${resolved} resolved, ${c.unresolved} unresolved`;
  const tooltip = [
    `calls to a function: ${c.to_function}`,
    `calls to a type: ${c.to_type}`,
    `external: ${c.external}`,
    `unresolved: ${c.unresolved}`,
    origin,
  ].join('\n');
  return { name, sources, nodes: graph.nodes.length, edges: graph.edges.length, resolved, unresolved: c.unresolved, text, tooltip };
}

/** A source file, as opposed to a build file (`pom.xml`, `Cargo.toml`). */
function isSourceFile(path: string): boolean {
  return path.endsWith('.java') || path.endsWith('.rs');
}

/** Why the chosen files cannot be indexed, or null. */
export function inputError(chosen: Chosen<unknown>): string | null {
  if (chosen.inputs.some((i) => isSourceFile(i.path))) return null;
  return `no Java or Rust sources found in ${chosen.root || 'the directory'} (only Java and Rust are supported now)`;
}
