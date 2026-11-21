// Work on many items with a few running at a time: the files of a forge (forge.ts) and the loose
// objects of a local `.git` (localGit.ts).

/** `work` on each item with at most `limit` running at a time; the results in the order of the items. Rejects on the first failure. */
export async function pool<T, R>(items: T[], limit: number, work: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  let failed = false;
  async function lane() {
    while (!failed && next < items.length) {
      const i = next++;
      try {
        out[i] = await work(items[i]);
      } catch (e) {
        failed = true;
        throw e;
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, lane));
  return out;
}
