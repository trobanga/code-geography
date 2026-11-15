import { collapse, type Tree } from './expansion';

/** The selected boxes, in the order of selection. */
export type Selection = readonly string[];

/**
 * The selection after a click on `id` (null: the background). A plain click selects that box
 * only. With `add` (Shift), the click adds the box or removes it, and the background keeps
 * the selection.
 */
export function pick(sel: Selection, id: string | null, add: boolean): string[] {
  if (!add) return id === null ? [] : [id];
  if (id === null) return [...sel];
  return sel.includes(id) ? sel.filter((s) => s !== id) : [...sel, id];
}

export type Strength = 'full' | 'weak' | 'dim';

/**
 * How strong the diagram draws a line for a selection. One selected box: its lines are full.
 * Two or more: the lines between them are full; a line to the outside is weak, or dim with
 * `betweenOnly`. All other lines are dim.
 */
export function lineStrength(sel: Selection, e: { from: string; to: string }, betweenOnly: boolean): Strength {
  if (sel.length === 0) return 'full';
  const ends = (sel.includes(e.from) ? 1 : 0) + (sel.includes(e.to) ? 1 : 0);
  if (ends === 2 || (ends === 1 && sel.length === 1)) return 'full';
  return ends === 1 && !betweenOnly ? 'weak' : 'dim';
}

/**
 * Open / close for a selection: the expanded set after the step, or null if nothing changes.
 * If a selected container is closed and can open (`expandable`, from the view), all such
 * containers open. If not, each open selected container closes with all below it.
 */
export function openOrClose(
  tree: Tree,
  expanded: ReadonlySet<string>,
  expandable: readonly string[],
  sel: Selection,
): Set<string> | null {
  const closed = sel.filter((id) => !expanded.has(id) && expandable.includes(id));
  if (closed.length > 0) return new Set([...expanded, ...closed]);
  if (!sel.some((id) => expanded.has(id))) return null;
  return sel.reduce<Set<string>>((out, id) => collapse(tree, out, id), new Set(expanded));
}
