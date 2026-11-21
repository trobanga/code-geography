import { describe, expect, it } from 'vitest';
import { jobRunner, type Wasm } from './workerJob';

/** A WASM that records what the runner asks of it; `overrides` replace single functions. */
function fakeWasm(overrides: Partial<Wasm> = {}) {
  const calls: string[] = [];
  const graph = JSON.stringify({ graph: { nodes: [], edges: [] }, report: {} });
  const wasm = {
    indexSources: () => graph,
    diffSources: () => graph,
    GitRepo: class {
      constructor(head: string) {
        calls.push(`repo ${head}`);
      }
      addRef(name: string) {
        calls.push(`ref ${name}`);
      }
      setPackedRefs(text: string) {
        calls.push(`packed ${text}`);
      }
      addPack(index: Uint8Array, data: Uint8Array) {
        calls.push(`pack ${index.length}+${data.length}`);
      }
      addLoose(id: string) {
        calls.push(`loose ${id}`);
      }
      log() {
        return '[]';
      }
      commitOverlay() {
        return graph;
      }
      free() {}
    },
    Worktree: class {
      json() {
        return graph;
      }
      free() {}
    },
    worktreeOverlay: () => graph,
    ...overrides,
  } as unknown as Wasm;
  return { wasm, calls };
}

describe('jobRunner with a fake WASM', () => {
  it('gives the packed refs, the packs and the loose objects of .git to the repository, in this order', () => {
    const { wasm, calls } = fakeWasm();
    const run = jobRunner(wasm);
    run({ kind: 'local', rootName: 'r', paths: [], texts: [] });

    const reply = run({
      kind: 'worktree',
      git: {
        head: 'ref: x',
        refs: [{ name: 'refs/heads/main', text: '1' }],
        packedRefs: 'packed',
        packs: [{ index: new Uint8Array(2), data: new Uint8Array(3) }],
        loose: [{ id: 'ab', bytes: new Uint8Array(1) }],
      },
    });

    expect(reply.ok).toBe(true);
    expect(calls).toEqual(['repo ref: x', 'ref refs/heads/main', 'packed packed', 'pack 2+3', 'loose ab']);
  });

  it('turns an error that is no Error object into the text of a failed reply', () => {
    const { wasm } = fakeWasm({
      indexSources: () => {
        throw 'plain text from the module';
      },
    });

    expect(jobRunner(wasm)({ kind: 'index', rootName: 'r', paths: [], texts: [] })).toEqual({ ok: false, error: 'plain text from the module' });
  });
});
