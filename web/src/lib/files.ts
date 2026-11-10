import { inTestCode } from './commit';
import type { Tree } from './expansion';
import type { ChangeCount, ChangedFile, Graph } from './types';

export interface FileEntry extends ChangedFile {
  /** A changed node has this file. */
  explained: boolean;
  /** The file has the extension of an indexed language (some node comes from such a file). */
  source: boolean;
  /** Path inside the directory of its module group; the full path outside modules. */
  rel: string;
}

export interface FileGroup {
  /** null: outside modules, or in a module without a box. */
  module: string | null;
  files: FileEntry[];
}

export interface ChangedFiles {
  /** Modules in order of first file, the group outside modules last. */
  groups: FileGroup[];
  totals: ChangeCount;
  /** Test files that the test-code switch hides. */
  hidden: number;
}

function extension(path: string): string {
  const name = path.slice(path.lastIndexOf('/') + 1);
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot) : '';
}

/** The changed files of a commit graph by module, for what the test-code switch shows (ADR 0012). */
export function changedFiles(graph: Graph, tree: Tree, tests: boolean): ChangedFiles {
  const explaining = new Set<string>();
  const exts = new Set<string>();
  for (const n of graph.nodes) {
    if (!n.file) continue;
    exts.add(extension(n.file));
    if (n.change) explaining.add(n.file);
  }
  const hasBox = (m: string | null) => m !== null && tree.byId.has(m) && (tests || !inTestCode(tree, m));
  const groups = new Map<string | null, FileEntry[]>();
  const outside: FileEntry[] = [];
  const totals: ChangeCount = { added: 0, removed: 0, modified: 0 };
  let hidden = 0;
  for (const file of graph.diff?.files ?? []) {
    if (!tests && file.test) {
      hidden++;
      continue;
    }
    totals[file.change]++;
    const entry = { ...file, explained: explaining.has(file.path), source: exts.has(extension(file.path)), rel: file.path };
    if (!hasBox(file.module)) {
      outside.push(entry);
      continue;
    }
    const dir = file.module!.startsWith('mod:') ? file.module!.slice(4) + '/' : null;
    if (dir && file.path.startsWith(dir)) entry.rel = file.path.slice(dir.length);
    const g = groups.get(file.module);
    if (g) g.push(entry);
    else groups.set(file.module, [entry]);
  }
  const out: FileGroup[] = [...groups].map(([module, files]) => ({ module, files }));
  if (outside.length > 0) out.push({ module: null, files: outside });
  return { groups: out, totals, hidden };
}
