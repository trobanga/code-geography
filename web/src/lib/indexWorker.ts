// Runs the indexer (ADR 0015) in its own WASM instance, so the page stays responsive.
import wasmUrl from './wasm/cg_wasm_bg.wasm?url';
import init, { diffSources, indexSources } from './wasm/cg_wasm.js';
import { runJob, type Job, type JobReply } from './workerJob';

self.onmessage = async (e: MessageEvent<Job>) => {
  let reply: JobReply;
  try {
    await init({ module_or_path: wasmUrl });
    reply = runJob(e.data, { indexSources, diffSources });
  } catch (err) {
    reply = { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
  self.postMessage(reply);
};
