// Main-thread side of the index worker: one worker per run, ended after the reply.
import type { CallReport } from './project';

export interface IndexRequest {
  paths: string[];
  texts: string[];
  rootName: string;
}
export type IndexReply = { ok: true; graphText: string; report: { calls: CallReport } } | { ok: false; error: string };

/** Indexes the given sources in a web worker; rejects with the message of the indexer or of a failed worker. */
export function indexInWorker(req: IndexRequest): Promise<{ graphText: string; report: { calls: CallReport } }> {
  return new Promise((resolve, reject) => {
    const w = new Worker(new URL('./indexWorker.ts', import.meta.url), { type: 'module' });
    w.onmessage = (e: MessageEvent<IndexReply>) => {
      w.terminate();
      const r = e.data;
      if (r.ok) resolve({ graphText: r.graphText, report: r.report });
      else reject(new Error(r.error));
    };
    w.onerror = (e) => {
      w.terminate();
      reject(new Error(`index worker: ${e.message || 'failed to start'}`));
    };
    w.postMessage(req);
  });
}
