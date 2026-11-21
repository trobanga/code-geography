import { describe, expect, it, vi } from 'vitest';
import { diffInWorker, indexInWorker, localRepoWorker, type WorkerLike } from './indexProject';
import type { GitFiles } from './localGit';
import type { WorkerRequest, WorkerResponse } from './workerJob';

/** A worker that the test answers by hand. */
class FakeWorker implements WorkerLike {
  posted: { msg: WorkerRequest; transfer: Transferable[] }[] = [];
  onmessage: ((e: { data: WorkerResponse }) => void) | null = null;
  onerror: ((e: { message: string }) => void) | null = null;
  terminated = false;
  postMessage(msg: WorkerRequest, transfer: Transferable[] = []) {
    this.posted.push({ msg, transfer });
  }
  terminate() {
    this.terminated = true;
  }
  reply(id: number, reply: WorkerResponse['reply']) {
    this.onmessage?.({ data: { id, reply } });
  }
}

const git = (): GitFiles => ({
  head: 'ref: refs/heads/main\n',
  refs: [],
  packedRefs: null,
  packs: [{ index: new Uint8Array(4), data: new Uint8Array(8) }],
  loose: [{ id: 'ab'.repeat(20), bytes: new Uint8Array(2) }],
});

const overlay = { ok: true, graphText: '{}', report: { calls: {} as never } } as const;

describe('localRepoWorker', () => {
  it('sends the sources as a local job and gives the plain index', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);

    const pending = repo.index({ rootName: 'r', paths: ['a'], texts: ['x'] });

    expect(w.posted).toHaveLength(1);
    expect(w.posted[0].msg.job).toEqual({ kind: 'local', rootName: 'r', paths: ['a'], texts: ['x'] });
    w.reply(w.posted[0].msg.id, overlay);
    await expect(pending).resolves.toEqual({ graphText: '{}', report: overlay.report });
  });

  it('sends the worktree job with the bytes of .git moved to the worker, and gives the overlay with the texts at HEAD', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);
    const files = git();
    const buffers = [files.packs[0].index.buffer, files.packs[0].data.buffer, files.loose[0].bytes.buffer];

    const pending = repo.worktree(files);

    expect(w.posted).toHaveLength(1);
    expect(w.posted[0].msg.job).toEqual({ kind: 'worktree', git: files });
    expect(w.posted[0].transfer).toEqual(buffers);
    w.reply(w.posted[0].msg.id, { ...overlay, base: { a: 'old' }, indexes: 1 });
    await expect(pending).resolves.toEqual({ graphText: '{}', report: overlay.report, base: { a: 'old' }, indexes: 1 });
  });

  it('matches the replies to the requests by number, in any order', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);
    const commit = { sha: 'a', parents: [], title: 't', author: 'x', date: '2026-01-01T00:00:00Z' };

    const log = repo.log(null, 30);
    const step = repo.overlay('b', 'h', 'r');

    expect(w.posted.map((p) => p.msg.job)).toEqual([
      { kind: 'log', from: null, n: 30 },
      { kind: 'commit', base: 'b', head: 'h', rootName: 'r' },
    ]);
    w.reply(w.posted[1].msg.id, { ...overlay, base: { a: 'b' }, head: { a: 'h' }, indexes: 2 });
    w.reply(w.posted[0].msg.id, { ok: true, commits: [commit] });
    await expect(step).resolves.toEqual({ graphText: '{}', report: overlay.report, base: { a: 'b' }, head: { a: 'h' }, indexes: 2 });
    await expect(log).resolves.toEqual([commit]);
  });

  it('rejects a request with the error of the reply', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);

    const log = repo.log('nope', 1);
    w.reply(w.posted[0].msg.id, { ok: false, error: 'no ref nope' });

    await expect(log).rejects.toThrow('no ref nope');
  });

  it('rejects all pending requests and later ones when the worker fails', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);
    const a = repo.log(null, 1);
    const b = repo.overlay(null, 'h', 'r');

    w.onerror?.({ message: 'out of memory' });

    await expect(a).rejects.toThrow('index worker: out of memory');
    await expect(b).rejects.toThrow('index worker: out of memory');
    await expect(repo.log(null, 1)).rejects.toThrow('index worker: out of memory');
  });

  it('ends the worker on close and rejects pending and later requests', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);
    const a = repo.log(null, 1);

    repo.close();

    expect(w.terminated).toBe(true);
    await expect(a).rejects.toThrow('closed');
    await expect(repo.overlay(null, 'h', 'r')).rejects.toThrow('closed');
  });
});

describe('localRepoWorker, replies without optional parts', () => {
  it('gives empty texts and no indexes for a worktree or commit reply that has none', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);

    const worktree = repo.worktree(git());
    w.reply(w.posted[0].msg.id, overlay);
    await expect(worktree).resolves.toEqual({ graphText: '{}', report: overlay.report, base: {}, indexes: 0 });

    const commit = repo.overlay(null, 'h', 'r');
    w.reply(w.posted[1].msg.id, overlay);
    await expect(commit).resolves.toEqual({ graphText: '{}', report: overlay.report, base: {}, head: {}, indexes: 0 });
  });

  it('rejects a log whose reply holds no commits', async () => {
    const w = new FakeWorker();
    const repo = localRepoWorker(() => w);

    const log = repo.log(null, 1);
    w.reply(w.posted[0].msg.id, overlay);

    await expect(log).rejects.toThrow('no commits in the reply');
  });
});

describe('indexInWorker and diffInWorker', () => {
  /** Stands in for `Worker`: answers each job at once and records it and the end of the worker. */
  class AnsweringWorker extends FakeWorker {
    static made: AnsweringWorker[] = [];
    constructor() {
      super();
      AnsweringWorker.made.push(this);
    }
    postMessage(msg: WorkerRequest, transfer: Transferable[] = []) {
      super.postMessage(msg, transfer);
      queueMicrotask(() => this.reply(msg.id, msg.job.kind === 'diff' ? { ok: false, error: 'diff broke' } : overlay));
    }
  }

  it('index the sources in a worker of its own that ends after the reply, and reject with the error of the reply', async () => {
    AnsweringWorker.made = [];
    vi.stubGlobal('Worker', AnsweringWorker);
    try {
      await expect(indexInWorker({ rootName: 'r', paths: ['a'], texts: ['x'] })).resolves.toMatchObject({ graphText: '{}', report: overlay.report });
      expect(AnsweringWorker.made[0].posted[0].msg.job).toEqual({ kind: 'index', rootName: 'r', paths: ['a'], texts: ['x'] });
      expect(AnsweringWorker.made[0].terminated).toBe(true);

      const side = { paths: [], texts: [] };
      await expect(diffInWorker({ rootName: 'r', base: side, head: side, baseName: 'b', headName: 'h', changed: [] })).rejects.toThrow('diff broke');
      expect(AnsweringWorker.made[1].terminated).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('start the real worker for a local repository when none is passed', async () => {
    AnsweringWorker.made = [];
    vi.stubGlobal('Worker', AnsweringWorker);
    try {
      const repo = localRepoWorker();
      await expect(repo.index({ rootName: 'r', paths: [], texts: [] })).resolves.toMatchObject({ graphText: '{}' });
      expect(AnsweringWorker.made).toHaveLength(1);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
