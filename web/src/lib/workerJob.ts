// The protocol of the index worker: one job per worker, run against the WASM functions.
import type { Changed, Side } from './forgeDiff';
import type { GitFiles } from './localGit';
import type { CallReport } from './project';

/** Index one source set (ADR 0015). */
export interface IndexJob {
  kind: 'index';
  paths: string[];
  texts: string[];
  rootName: string;
}

/** The overlay of two source sets (ADR 0009, 0012, 0021). */
export interface DiffJob {
  kind: 'diff';
  rootName: string;
  base: Side;
  head: Side;
  baseName: string;
  headName: string;
  changed: Changed[];
}

/** The overlay of the working tree of a local repository against `HEAD` (ADR 0039). */
export interface WorktreeJob {
  kind: 'worktree';
  rootName: string;
  paths: string[];
  texts: string[];
  git: GitFiles;
}

export type Job = IndexJob | DiffJob | WorktreeJob;
/** `base`: of a worktree job, the texts of the inputs at `HEAD` by path. */
export type JobReply =
  | { ok: true; graphText: string; report: { calls: CallReport }; base?: Record<string, string> }
  | { ok: false; error: string };

export interface Wasm {
  indexSources(paths: string[], texts: string[], rootName: string): string;
  diffSources(
    basePaths: string[],
    baseTexts: string[],
    headPaths: string[],
    headTexts: string[],
    rootName: string,
    baseName: string,
    headName: string,
    changedPaths: string[],
    changedKinds: string[],
  ): string;
  worktreeOverlay(git: GitFiles, paths: string[], texts: string[], rootName: string): string;
}

/** The parts of the class `GitRepo` of the WASM module that `worktreeOf` uses. */
export interface GitRepoLike {
  addRef(name: string, text: string): void;
  setPackedRefs(text: string): void;
  addPack(index: Uint8Array, data: Uint8Array): void;
  addLoose(id: string, bytes: Uint8Array): void;
  free(): void;
}

/** `Wasm.worktreeOverlay` from the class `GitRepo` and the function `worktreeOverlay` of the module. */
export function worktreeOf<G extends GitRepoLike>(
  Repo: new (head: string) => G,
  overlay: (repo: G, paths: string[], texts: string[], rootName: string) => string,
): Wasm['worktreeOverlay'] {
  return (git, paths, texts, rootName) => {
    const repo = new Repo(git.head);
    try {
      for (const r of git.refs) repo.addRef(r.name, r.text);
      if (git.packedRefs !== null) repo.setPackedRefs(git.packedRefs);
      for (const p of git.packs) repo.addPack(p.index, p.data);
      for (const l of git.loose) repo.addLoose(l.id, l.bytes);
      return overlay(repo, paths, texts, rootName);
    } finally {
      repo.free();
    }
  };
}

/** Runs the job; an error of the indexer becomes a failed reply. */
export function runJob(job: Job, wasm: Wasm): JobReply {
  try {
    const json =
      job.kind === 'index'
        ? wasm.indexSources(job.paths, job.texts, job.rootName)
        : job.kind === 'worktree'
          ? wasm.worktreeOverlay(job.git, job.paths, job.texts, job.rootName)
          : wasm.diffSources(
            job.base.paths,
            job.base.texts,
            job.head.paths,
            job.head.texts,
            job.rootName,
            job.baseName,
            job.headName,
            job.changed.map((c) => c.path),
            job.changed.map((c) => c.change),
          );
    const { graph, report, base } = JSON.parse(json);
    return { ok: true, graphText: JSON.stringify(graph), report, ...(base ? { base } : {}) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}
