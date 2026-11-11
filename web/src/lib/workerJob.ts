// The protocol of the index worker: one job per worker, run against the WASM functions.
import type { Changed, Side } from './forgeDiff';
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

export type Job = IndexJob | DiffJob;
export type JobReply = { ok: true; graphText: string; report: { calls: CallReport } } | { ok: false; error: string };

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
}

/** Runs the job; an error of the indexer becomes a failed reply. */
export function runJob(job: Job, wasm: Wasm): JobReply {
  try {
    const json =
      job.kind === 'index'
        ? wasm.indexSources(job.paths, job.texts, job.rootName)
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
    const { graph, report } = JSON.parse(json);
    return { ok: true, graphText: JSON.stringify(graph), report };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}
