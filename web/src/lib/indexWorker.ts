// Runs the indexer (ADR 0015) in its own WASM instance, so the page stays responsive. One runner
// per worker: a local repository keeps its git repository here between jobs (ADR 0040).
import wasmUrl from './wasm/cg_wasm_bg.wasm?url';
import init, { GitRepo, diffSources, indexSources, worktreeOverlay } from './wasm/cg_wasm.js';
import { jobRunner, type JobReply, type WorkerRequest } from './workerJob';

const ready = init({ module_or_path: wasmUrl });
ready.catch(() => {}); // each job reports the failure itself
const run = jobRunner({ indexSources, diffSources, GitRepo, worktreeOverlay });
/** Jobs run one after the other, in the order of the messages. */
let queue: Promise<unknown> = Promise.resolve();

self.onmessage = (e: MessageEvent<WorkerRequest>) => {
  const { id, job } = e.data;
  queue = queue.then(async () => {
    let reply: JobReply;
    try {
      await ready;
      reply = run(job);
    } catch (err) {
      reply = { ok: false, error: err instanceof Error ? err.message : String(err) };
    }
    self.postMessage({ id, reply });
  });
};
