// Main-thread side of the index worker: one worker per run, ended after the reply.
import type { CallReport } from './project';
import type { DiffJob, IndexJob, Job, JobReply } from './workerJob';

export type IndexRequest = Omit<IndexJob, 'kind'>;
export type DiffRequest = Omit<DiffJob, 'kind'>;
type Indexed = { graphText: string; report: { calls: CallReport } };

/** Indexes the given sources in a web worker; rejects with the message of the indexer or of a failed worker. */
export function indexInWorker(req: IndexRequest): Promise<Indexed> {
  return inWorker({ kind: 'index', ...req });
}

/** The commit overlay of two source sets, built in a web worker (ADR 0021). */
export function diffInWorker(req: DiffRequest): Promise<Indexed> {
  return inWorker({ kind: 'diff', ...req });
}

function inWorker(job: Job): Promise<Indexed> {
  return new Promise((resolve, reject) => {
    const w = new Worker(new URL('./indexWorker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<JobReply>) => {
      w.terminate();
      const r = e.data;
      if (r.ok) resolve({ graphText: r.graphText, report: r.report });
      else reject(new Error(r.error));
    };
    w.onerror = (e) => {
      w.terminate();
      reject(new Error(`index worker: ${e.message || 'failed to start'}`));
    };
    w.postMessage(job);
  });
}
