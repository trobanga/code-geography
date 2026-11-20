// Runs the indexer (ADR 0015) in its own WASM instance, so the page stays responsive.
import wasmUrl from './wasm/cg_wasm_bg.wasm?url';
import init, { GitRepo, diffSources, indexSources, worktreeOverlay } from './wasm/cg_wasm.js';
import { runJob, worktreeOf, type Job, type JobReply } from './workerJob';

self.onmessage = async (e: MessageEvent<Job>) => {
  let reply: JobReply;
  try {
    await init({ module_or_path: wasmUrl });
    reply = runJob(e.data, { indexSources, diffSources, worktreeOverlay: worktreeOf(GitRepo, worktreeOverlay) });
  } catch (err) {
    reply = { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
  self.postMessage(reply);
};
