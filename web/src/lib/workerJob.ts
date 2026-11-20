// The protocol of the index worker: one job per worker, run against the WASM functions.
import type { Commit } from './forge';
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

/** The log of the kept repository: the first-parent history from `from` (null: `HEAD`), at most `n` commits (ADR 0040). */
export interface LogJob {
  kind: 'log';
  from: string | null;
  n: number;
}

/** The overlay of the tree `head` of the kept repository against the tree `base` (null: nothing). */
export interface CommitJob {
  kind: 'commit';
  base: string | null;
  head: string;
  rootName: string;
}

export type Job = IndexJob | DiffJob | WorktreeJob | LogJob | CommitJob;
/** An overlay or a plain index. `base`: of a worktree or commit job, the texts of the inputs at the base by path. */
export type GraphReply = {
  ok: true;
  graphText: string;
  report: { calls: CallReport };
  base?: Record<string, string>;
  head?: Record<string, string>;
  indexes?: number;
};
export type Failed = { ok: false; error: string };
export type JobReply = GraphReply | { ok: true; commits: Commit[] } | Failed;

/** The reply that a job gets: a log gives commits, a commit job an overlay with both sides, any other an overlay or an index. */
export type ReplyOf<J extends Job> = J extends LogJob ? { ok: true; commits: Commit[] } | Failed : J extends CommitJob ? CommitReply | Failed : GraphReply | Failed;

/** A message to a worker: a job with a number, which the reply echoes. */
export interface WorkerRequest {
  id: number;
  job: Job;
}

export interface WorkerResponse {
  id: number;
  reply: JobReply;
}

/** The reply of a commit job. */
export type CommitReply = GraphReply & { base: Record<string, string>; head: Record<string, string>; indexes: number };

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
  /** The class `GitRepo` of the WASM module. */
  GitRepo: new (head: string) => GitRepoLike;
  worktreeOverlay(repo: GitRepoLike, paths: string[], texts: string[], rootName: string): string;
}

/** The parts of the class `GitRepo` of the WASM module that the runner uses. */
export interface GitRepoLike {
  addRef(name: string, text: string): void;
  setPackedRefs(text: string): void;
  addPack(index: Uint8Array, data: Uint8Array): void;
  addLoose(id: string, bytes: Uint8Array): void;
  log(from: string | null | undefined, n: number): string;
  commitOverlay(base: string | null | undefined, head: string, rootName: string): string;
  free(): void;
}

/**
 * Runs jobs against the WASM functions. It is stateful: a worktree job builds the repository
 * and keeps it (a previous one is freed), so that log and commit jobs go on from it (ADR 0040).
 * An error of the indexer becomes a failed reply.
 */
export function jobRunner(wasm: Wasm): <J extends Job>(job: J) => ReplyOf<J> {
  let repo: GitRepoLike | null = null;
  const graphReply = (json: string): GraphReply => {
    const { graph, report, base, head, indexes } = JSON.parse(json);
    return { ok: true, graphText: JSON.stringify(graph), report, ...(base ? { base } : {}), ...(head ? { head } : {}), ...(indexes !== undefined ? { indexes } : {}) };
  };
  const kept = () => {
    if (!repo) throw new Error('no repository');
    return repo;
  };
  const run = (job: Job): JobReply => {
    try {
      switch (job.kind) {
        case 'index':
          return graphReply(wasm.indexSources(job.paths, job.texts, job.rootName));
        case 'diff':
          return graphReply(
            wasm.diffSources(
              job.base.paths,
              job.base.texts,
              job.head.paths,
              job.head.texts,
              job.rootName,
              job.baseName,
              job.headName,
              job.changed.map((c) => c.path),
              job.changed.map((c) => c.change),
            ),
          );
        case 'worktree': {
          repo?.free();
          repo = null;
          const made = new wasm.GitRepo(job.git.head);
          try {
            for (const r of job.git.refs) made.addRef(r.name, r.text);
            if (job.git.packedRefs !== null) made.setPackedRefs(job.git.packedRefs);
            for (const p of job.git.packs) made.addPack(p.index, p.data);
            for (const l of job.git.loose) made.addLoose(l.id, l.bytes);
            const reply = graphReply(wasm.worktreeOverlay(made, job.paths, job.texts, job.rootName));
            repo = made;
            return reply;
          } catch (err) {
            made.free();
            throw err;
          }
        }
        case 'log':
          return { ok: true, commits: JSON.parse(kept().log(job.from, job.n)) as Commit[] };
        case 'commit':
          return graphReply(kept().commitOverlay(job.base, job.head, job.rootName));
      }
    } catch (err) {
      return { ok: false, error: err instanceof Error ? err.message : String(err) };
    }
  };
  return run as <J extends Job>(job: J) => ReplyOf<J>;
}
