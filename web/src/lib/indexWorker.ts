// Runs the Java indexer (ADR 0015) in its own WASM instance, so the page stays responsive.
import wasmUrl from './wasm/cg_wasm_bg.wasm?url';
import init, { indexJava } from './wasm/cg_wasm.js';
import type { IndexReply, IndexRequest } from './indexProject';

self.onmessage = async (e: MessageEvent<IndexRequest>) => {
  const { paths, texts, rootName } = e.data;
  let reply: IndexReply;
  try {
    await init({ module_or_path: wasmUrl });
    const { graph, report } = JSON.parse(indexJava(paths, texts, rootName));
    reply = { ok: true, graphText: JSON.stringify(graph), report };
  } catch (err) {
    reply = { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
  self.postMessage(reply);
};
