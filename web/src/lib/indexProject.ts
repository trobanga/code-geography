// Main-thread side of the index worker: one worker per run, ended after the reply, or one worker
// that a local repository keeps for its history (ADR 0040).
import type { Commit } from './forge';
import type { CallReport } from './project';
import type { CommitJob, DiffJob, IndexJob, Job, JobReply, WorkerRequest, WorkerResponse, WorktreeJob } from './workerJob';

export type IndexRequest = Omit<IndexJob, 'kind'>;
export type DiffRequest = Omit<DiffJob, 'kind'>;
export type WorktreeRequest = Omit<WorktreeJob, 'kind'>;
type Indexed = { graphText: string; report: { calls: CallReport } };

/** The part of `Worker` that is used; a test passes a fake. */
export interface WorkerLike {
  postMessage(msg: WorkerRequest, transfer?: Transferable[]): void;
  onmessage: ((e: { data: WorkerResponse }) => void) | null;
  onerror: ((e: { message: string }) => void) | null;
  terminate(): void;
}

const startWorker = (): WorkerLike => new Worker(new URL('./indexWorker.ts', import.meta.url), { type: 'module' }) as unknown as WorkerLike;

/** Indexes the given sources in a web worker; rejects with the message of the indexer or of a failed worker. */
export function indexInWorker(req: IndexRequest): Promise<Indexed> {
  return once({ kind: 'index', ...req });
}

/** The commit overlay of two source sets, built in a web worker (ADR 0021). */
export function diffInWorker(req: DiffRequest): Promise<Indexed> {
  return once({ kind: 'diff', ...req });
}

/** The bytes of the packs and loose objects of `.git`: they move to the worker, so they are unusable here afterwards. */
const bytesOf = (git: WorktreeRequest['git']) =>
  [...git.packs.flatMap((p) => [p.index.buffer, p.data.buffer]), ...git.loose.map((l) => l.bytes.buffer)] as ArrayBuffer[];

type Done = Extract<JobReply, { ok: true }>;
type Graph = Extract<Done, { graphText: string }>;

function once(job: Job): Promise<Graph> {
  const channel = openChannel(startWorker());
  return channel.request(job).then(
    (r) => {
      channel.close();
      return r as Graph;
    },
    (e) => {
      channel.close();
      throw e;
    },
  );
}

/** Jobs numbered, replies matched by number; an error of the worker rejects what is pending and all later jobs. */
function openChannel(w: WorkerLike) {
  const pending = new Map<number, { resolve: (r: Done) => void; reject: (e: Error) => void }>();
  let next = 0;
  let failure: Error | null = null;
  const fail = (e: Error) => {
    failure ??= e;
    for (const p of pending.values()) p.reject(failure);
    pending.clear();
  };
  w.onmessage = (e) => {
    const p = pending.get(e.data.id);
    if (!p) return;
    pending.delete(e.data.id);
    if (e.data.reply.ok) p.resolve(e.data.reply);
    else p.reject(new Error(e.data.reply.error));
  };
  w.onerror = (e) => fail(new Error(`index worker: ${e.message || 'failed to start'}`));
  return {
    request(job: Job, transfer: Transferable[] = []): Promise<Done> {
      if (failure) return Promise.reject(failure);
      return new Promise((resolve, reject) => {
        const id = next++;
        pending.set(id, { resolve, reject });
        w.postMessage({ id, job }, transfer);
      });
    },
    close() {
      fail(new Error('the repository was closed'));
      w.terminate();
    },
  };
}

/** What the viewer shows of a commit overlay of a local repository. */
export type CommitOverlay = Indexed & { base: Record<string, string>; head: Record<string, string>; indexes: number };

/** One worker that keeps the git repository of a chosen directory and answers for its history (ADR 0040). */
export interface LocalRepoWorker {
  /** The overlay of the working tree against `HEAD`; the worker keeps the repository. The bytes of `.git` move to it. */
  worktree(req: WorktreeRequest): Promise<Indexed & { base: Record<string, string>; indexes: number }>;
  /** The first-parent history from `from` (null: `HEAD`), at most `n` commits. */
  log(from: string | null, n: number): Promise<Commit[]>;
  /** The overlay of the tree `head` against the tree `base` (null: nothing). */
  overlay(base: string | null, head: string, rootName: string): Promise<CommitOverlay>;
  close(): void;
}

export function localRepoWorker(make: () => WorkerLike = startWorker): LocalRepoWorker {
  const channel = openChannel(make());
  const graph = (r: Done) => r as Graph;
  return {
    worktree: async (req) => {
      const r = graph(await channel.request({ kind: 'worktree', ...req }, bytesOf(req.git)));
      return { graphText: r.graphText, report: r.report, base: r.base ?? {}, indexes: r.indexes ?? 0 };
    },
    log: async (from, n) => {
      const r = await channel.request({ kind: 'log', from, n });
      if (!('commits' in r)) throw new Error('index worker: no commits in the reply');
      return r.commits;
    },
    overlay: async (base, head, rootName) => {
      const job: CommitJob = { kind: 'commit', base, head, rootName };
      const r = graph(await channel.request(job));
      return { graphText: r.graphText, report: r.report, base: r.base ?? {}, head: r.head ?? {}, indexes: r.indexes ?? 0 };
    },
    close: channel.close,
  };
}
