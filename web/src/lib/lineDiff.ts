/** One line of a line diff, with 1-based line numbers. A removed line has `head: null`, an added line `base: null`. */
export interface DiffLine {
  base: number | null;
  head: number | null;
}

/** Above this many changed lines between the common start and the common end, that part counts as replaced. */
const MAX_EDITS = 1000;

type Op = 'keep' | 'remove' | 'add';

/**
 * The line diff of two texts given as lines: a shortest edit script (Myers). The result lists
 * every line of both sides once, in the order of the files. In each run of changes the removed
 * lines come before the added ones. If the part between the common start and the common end
 * needs more than `maxEdits` edits, that part is all removed, then all added.
 */
export function lineDiff(base: readonly string[], head: readonly string[], maxEdits = MAX_EDITS): DiffLine[] {
  let start = 0;
  while (start < base.length && start < head.length && base[start] === head[start]) start++;
  let endBase = base.length;
  let endHead = head.length;
  while (endBase > start && endHead > start && base[endBase - 1] === head[endHead - 1]) {
    endBase--;
    endHead--;
  }
  const middleBase = base.slice(start, endBase);
  const middleHead = head.slice(start, endHead);
  const ops = myers(middleBase, middleHead, maxEdits) ?? replaced(middleBase.length, middleHead.length);

  const out: DiffLine[] = [];
  for (let i = 0; i < start; i++) out.push({ base: i + 1, head: i + 1 });
  let b = start;
  let h = start;
  for (const op of ops) {
    if (op === 'keep') out.push({ base: ++b, head: ++h });
    else if (op === 'remove') out.push({ base: ++b, head: null });
    else out.push({ base: null, head: ++h });
  }
  for (let i = 0; i < base.length - endBase; i++) out.push({ base: endBase + i + 1, head: endHead + i + 1 });
  return out;
}

function replaced(removed: number, added: number): Op[] {
  return [...Array<Op>(removed).fill('remove'), ...Array<Op>(added).fill('add')];
}

/**
 * The greedy O(ND) algorithm of Myers with the trace of each round kept for the way back.
 * Null when the script needs more than `maxEdits` edits.
 */
function myers(a: readonly string[], b: readonly string[], maxEdits: number): Op[] | null {
  const n = a.length;
  const m = b.length;
  const limit = Math.min(n + m, maxEdits);
  const offset = limit + 1;
  const v = new Int32Array(2 * limit + 3);
  const trace: Int32Array[] = [];
  for (let d = 0; d <= limit; d++) {
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1]) ? v[offset + k + 1] : v[offset + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) return script(trace, n, m, offset);
    }
  }
  return null;
}

/** Walks the trace back from the end of both texts; each run of changes gets its removals first. */
function script(trace: Int32Array[], n: number, m: number, offset: number): Op[] {
  const reversed: Op[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d];
    const k = x - y;
    const previousK = k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1]) ? k + 1 : k - 1;
    const previousX = v[offset + previousK];
    const previousY = previousX - previousK;
    while (x > previousX && y > previousY) {
      reversed.push('keep');
      x--;
      y--;
    }
    if (d > 0) reversed.push(x === previousX ? 'add' : 'remove');
    x = previousX;
    y = previousY;
  }
  const ops = reversed.reverse();
  // Within a run of changes, the removals go first.
  let i = 0;
  while (i < ops.length) {
    if (ops[i] === 'keep') {
      i++;
      continue;
    }
    let end = i;
    while (end < ops.length && ops[end] !== 'keep') end++;
    const run = ops.slice(i, end);
    const removed = run.filter((op) => op === 'remove').length;
    ops.splice(i, end - i, ...replaced(removed, run.length - removed));
    i = end;
  }
  return ops;
}
