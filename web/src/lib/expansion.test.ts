import { describe, expect, it } from 'vitest';
import { BOX_LIMIT, buildTree, mustConfirm, collapse, deeper, drawable, shallower, stepBack, stepForward, levelExpanded, levelUnfolded, splitByLevel } from './expansion';
import type { GraphNode } from './types';

const nodes: GraphNode[] = [
  { id: 'm1', parent: null, kind: 'module', name: 'm1' },
  { id: 'm2', parent: null, kind: 'module', name: 'm2' },
  { id: 'p1', parent: 'm1', kind: 'package', name: 'p1' },
  { id: 'p2', parent: 'm1', kind: 'package', name: 'p2' },
  { id: 't1', parent: 'p1', kind: 'type', name: 'T1' },
  { id: 't1i', parent: 't1', kind: 'type', name: 'Inner' },
  { id: 'f1', parent: 't1', kind: 'function', name: 'f1' },
  { id: 'f2', parent: 't1i', kind: 'function', name: 'f2' },
  { id: 't2', parent: 'p2', kind: 'type', name: 'T2' },
];
const tree = buildTree(nodes);
const sorted = (s: Iterable<string>) => [...s].sort();
/** Nested packages: p holds p.q and the type A; p.q holds p.q.r (test code only) and B. */
const nested = buildTree([
  { id: 'm', parent: null, kind: 'module', name: 'm' },
  { id: 'p', parent: 'm', kind: 'package', name: 'p' },
  { id: 'A', parent: 'p', kind: 'type', name: 'A' },
  { id: 'p.q', parent: 'p', kind: 'package', name: 'q' },
  { id: 'B', parent: 'p.q', kind: 'type', name: 'B' },
  { id: 'B.f', parent: 'B', kind: 'function', name: 'f' },
  { id: 'p.q.r', parent: 'p.q', kind: 'package', name: 'r' },
  { id: 'R', parent: 'p.q.r', kind: 'type', name: 'R', test: true },
  { id: 'p.s', parent: 'p', kind: 'package', name: 's' },
  { id: 'C', parent: 'p.s', kind: 'type', name: 'C' },
]);

describe('levelExpanded', () => {
  it('modules: nothing expanded', () => {
    expect(sorted(levelExpanded(tree, 'modules'))).toEqual([]);
  });
  it('packages: modules with children expanded', () => {
    expect(sorted(levelExpanded(tree, 'packages'))).toEqual(['m1']);
  });
  it('packages: also every package with a package child that the view can show', () => {
    expect(sorted(levelExpanded(nested, 'packages'))).toEqual(['m', 'p', 'p.q']);
    expect(sorted(levelExpanded(nested, 'packages', new Set(['m', 'p', 'p.q', 'A', 'B', 'p.s', 'C'])))).toEqual(['m', 'p']);
  });
  it('types: modules and packages with children expanded', () => {
    expect(sorted(levelExpanded(tree, 'types'))).toEqual(['m1', 'p1', 'p2']);
  });
  it('functions: modules, packages and types (nested too) with children expanded', () => {
    expect(sorted(levelExpanded(tree, 'functions'))).toEqual(['m1', 'p1', 'p2', 't1', 't1i']);
  });
});

describe('levelUnfolded', () => {
  it('unfolds every container with packages and other children from the level types on', () => {
    expect(sorted(levelUnfolded(nested, 'packages'))).toEqual([]);
    expect(sorted(levelUnfolded(nested, 'types'))).toEqual(['p', 'p.q']);
    expect(sorted(levelUnfolded(nested, 'functions'))).toEqual(['p', 'p.q']);
  });
  it('skips a container whose other children the view cannot show', () => {
    const shown = new Set(['m', 'p', 'p.q', 'p.q.r', 'p.s', 'C']);
    expect(sorted(levelUnfolded(nested, 'types', shown))).toEqual([]);
  });
  it('skips a container whose package children the view cannot show', () => {
    const shown = new Set(['m', 'p', 'A', 'p.q', 'B', 'B.f', 'p.s', 'C']);
    expect(sorted(levelUnfolded(nested, 'types', shown))).toEqual(['p']);
  });
});

describe('splitByLevel with the kind fold', () => {
  it('needs the unfolded set of the level and returns the extra unfolded ids', () => {
    const ex = levelExpanded(nested, 'types');
    expect(splitByLevel(nested, ex, undefined, new Set(['p']))).toEqual({ level: 'packages', open: ['p.q.r', 'p.s'], unfold: ['p'] });
    expect(splitByLevel(nested, ex, undefined, new Set(['p', 'p.q']))).toEqual({ level: 'types', open: [], unfold: [] });
  });
});

describe('splitByLevel', () => {
  it('finds the deepest contained level and the extra open ids', () => {
    expect(splitByLevel(tree, new Set(['m1', 'p1', 'p2', 't1']))).toEqual({ level: 'types', open: ['t1'], unfold: [] });
  });
  it('falls back to modules when a level node was collapsed', () => {
    expect(splitByLevel(tree, new Set(['p1']))).toEqual({ level: 'modules', open: ['p1'], unfold: [] });
  });
  it('ignores level containers that the view does not show, e.g. ones with only test code', () => {
    const shown = new Set(['m1', 'm2', 'p1', 't1', 't1i', 'f1', 'f2']);
    expect(splitByLevel(tree, new Set(['m1', 'p1']), shown)).toEqual({ level: 'types', open: [], unfold: [] });
  });
  it('round-trips with levelExpanded', () => {
    const expanded = new Set([...levelExpanded(tree, 'packages'), 't1']);
    const { level, open } = splitByLevel(tree, expanded);
    expect(sorted(new Set([...levelExpanded(tree, level), ...open]))).toEqual(sorted(expanded));
  });
});

describe('labels', () => {
  it('drops the dot-prefix shared by all package names, keeps other names', () => {
    const t = buildTree([
      { id: 'm', parent: null, kind: 'module', name: 'm' },
      { id: 'a', parent: 'm', kind: 'package', name: 'care.smith.fts.api' },
      { id: 'b', parent: 'm', kind: 'package', name: 'care.smith.fts.api.cda' },
      { id: 'c', parent: 'm', kind: 'package', name: 'care.smith.fts.util' },
      { id: 'T', parent: 'a', kind: 'type', name: 'care.Thing' },
    ]);
    expect(t.label.get('a')).toBe('api');
    expect(t.label.get('b')).toBe('api.cda');
    expect(t.label.get('c')).toBe('util');
    expect(t.label.get('T')).toBe('care.Thing');
  });
  it('keeps a package name that equals the shared prefix', () => {
    const t = buildTree([
      { id: 'a', parent: null, kind: 'package', name: 'x.y' },
      { id: 'b', parent: null, kind: 'package', name: 'x.y.z' },
    ]);
    expect(t.label.get('a')).toBe('x.y');
    expect(t.label.get('b')).toBe('z');
  });
  it('takes the shared prefix from the top-level packages only, and leaves a nested package name as it is', () => {
    const t = buildTree([
      { id: 'm1', parent: null, kind: 'module', name: 'm1' },
      { id: 'm2', parent: null, kind: 'module', name: 'm2' },
      { id: 'a', parent: 'm1', kind: 'package', name: 'care.smith.fts.cda' },
      { id: 'b', parent: 'a', kind: 'package', name: 'impl' },
      { id: 'c', parent: 'm2', kind: 'package', name: 'care.smith.fts.util' },
    ]);
    expect(t.label.get('a')).toBe('cda');
    expect(t.label.get('b')).toBe('impl');
    expect(t.label.get('c')).toBe('util');
  });
});

describe('collapse', () => {
  it('removes the node and its expanded descendants, keeps the rest', () => {
    const expanded = new Set(['m1', 'p1', 't1', 't1i', 'p2', 'm2']);
    expect(sorted(collapse(tree, expanded, 'p1'))).toEqual(['m1', 'm2', 'p2']);
  });
  it('does not mutate its input', () => {
    const expanded = new Set(['m1', 'p1']);
    collapse(tree, expanded, 'm1');
    expect(sorted(expanded)).toEqual(['m1', 'p1']);
  });
});

describe('drawable with a placeholder of the focus cut', () => {
  it('treats rest:<c> as a child of c', () => {
    expect(drawable(tree, { from: 'rest:p1', to: 'm1' })).toBe(false);
    expect(drawable(tree, { from: 'm1', to: 'rest:p1' })).toBe(false);
    expect(drawable(tree, { from: 'rest:p1', to: 't2' })).toBe(true);
    expect(drawable(tree, { from: 'rest:p1', to: 'rest:p2' })).toBe(true);
  });
});

describe('deeper with the kind fold', () => {
  it('opens the leaf packages and unfolds the type placeholders in one step', () => {
    const view = { nodes: ['m', 'p', 'p.q', 'p.s', 'p.q.r', 'rest:p', 'rest:p.q'], expandable: ['p.s', 'p.q.r', 'rest:p', 'rest:p.q'] };
    const r = deeper(nested, view, { expanded: new Set(['m', 'p', 'p.q']), unfolded: new Set() });
    expect(sorted(r.expanded)).toEqual(['m', 'p', 'p.q', 'p.q.r', 'p.s']);
    expect(sorted(r.unfolded)).toEqual(['p', 'p.q']);
  });
  it('opens a closed container with package children before it unfolds a type placeholder', () => {
    const view = { nodes: ['m', 'p', 'p.q', 'p.s', 'rest:p'], expandable: ['p.q', 'p.s', 'rest:p'] };
    const r = deeper(nested, view, { expanded: new Set(['m', 'p']), unfolded: new Set() });
    expect(sorted(r.expanded)).toEqual(['m', 'p', 'p.q']);
    expect(sorted(r.unfolded)).toEqual([]);
  });
  it('skips a placeholder of the focus: its container shows a type or folds a package', () => {
    const view = { nodes: ['m', 'p', 'A', 'rest:p'], expandable: ['rest:p'] };
    const r = deeper(nested, view, { expanded: new Set(['m', 'p']), unfolded: new Set() });
    expect(sorted(r.unfolded)).toEqual([]);
  });
});

describe('deeper', () => {
  it('ranks a type as function, so that types with inner types open with the other types', () => {
    const t = buildTree([
      { id: 'm', parent: null, kind: 'module', name: 'm' },
      { id: 'p', parent: 'm', kind: 'package', name: 'p' },
      { id: 'A', parent: 'p', kind: 'type', name: 'A' },
      { id: 'B', parent: 'A', kind: 'type', name: 'B' },
      { id: 'b', parent: 'B', kind: 'function', name: 'b' },
      { id: 'C', parent: 'p', kind: 'type', name: 'C' },
      { id: 'c', parent: 'C', kind: 'function', name: 'c' },
    ]);
    expect(sorted(deeper(t, { nodes: [], expandable: ['m', 'p', 'A', 'C'] }, { expanded: new Set(['m', 'p']), unfolded: new Set() }).expanded)).toEqual(['A', 'C', 'm', 'p']);
  });
  it('ranks a container by the children that the view can show', () => {
    // Without its hidden inner type, t1 holds only a function and waits for p2.
    const shown = new Set(['m1', 'm2', 'p1', 'p2', 't1', 'f1', 't2']);
    expect(sorted(deeper(tree, { nodes: [], expandable: ['m1', 'p1', 'p2', 't1'] }, { expanded: new Set(['m1', 'p1']), unfolded: new Set() }, shown).expanded)).toEqual(['m1', 'p1', 'p2']);
  });
  it('opens only the frontier containers whose children have the coarsest kind', () => {
    // p2 holds a type, t1i only a function: the type layer opens first.
    expect(sorted(deeper(tree, { nodes: [], expandable: ['m1', 'p1', 'p2', 't1', 't1i'] }, { expanded: new Set(['m1', 'p1', 't1']), unfolded: new Set() }).expanded)).toEqual([
      'm1',
      'p1',
      'p2',
      't1',
    ]);
  });
  it('ranks a value as a function: a package of values opens with the function layer', () => {
    const t = buildTree([
      { id: 'm', parent: null, kind: 'module', name: 'm' },
      { id: 'p', parent: 'm', kind: 'package', name: 'p' },
      { id: 'A', parent: 'p', kind: 'type', name: 'A' },
      { id: 'a', parent: 'A', kind: 'function', name: 'a' },
      { id: 'q', parent: 'm', kind: 'package', name: 'q' },
      { id: 'v', parent: 'q', kind: 'value', name: 'v' },
    ]);
    const step = (expanded: string[], expandable: string[]) =>
      sorted(deeper(t, { nodes: [], expandable }, { expanded: new Set(expanded), unfolded: new Set() }).expanded);
    expect(step(['m'], ['p', 'q'])).toEqual(['m', 'p']);
    expect(step(['m', 'p'], ['q', 'A'])).toEqual(['A', 'm', 'p', 'q']);
  });
  it('opens every visible closed container of the view, skips placeholders', () => {
    expect(sorted(deeper(tree, { nodes: [], expandable: ['p1', 'p2', 'rest:m1'] }, { expanded: new Set(['m1']), unfolded: new Set() }).expanded)).toEqual(['m1', 'p1', 'p2']);
  });
});

const only = (...ids: string[]) => ({ expanded: new Set(ids), unfolded: new Set<string>() });

describe('shallower', () => {
  it('closes every open container without an open descendant', () => {
    expect(sorted(shallower(tree, only('m1', 'p1', 't1', 'p2', 'm2')).expanded)).toEqual(['m1', 'p1']);
  });
  it('drops open ids below a closed container, which the cut does not show', () => {
    expect(sorted(shallower(tree, only('m1', 't1')).expanded)).toEqual([]);
  });
});

describe('shallower with the kind fold', () => {
  it('folds the unfolded containers again whose folded kind is as fine as the closed layer', () => {
    const types = { expanded: new Set(['m', 'p', 'p.q', 'p.q.r', 'p.s']), unfolded: new Set(['p', 'p.q']) };
    const r = shallower(nested, types);
    expect(sorted(r.expanded)).toEqual(['m', 'p', 'p.q']);
    expect(sorted(r.unfolded)).toEqual([]);
  });
  it('treats an open container whose open children the view cannot show as a leaf', () => {
    const shown = new Set(['m', 'p', 'A', 'p.q', 'B', 'B.f', 'p.s', 'C']);
    const types = { expanded: new Set(['m', 'p', 'p.q', 'p.q.r', 'p.s']), unfolded: new Set(['p']) };
    const r = shallower(nested, types, shown);
    expect(sorted(r.expanded)).toEqual(['m', 'p']);
    expect(sorted(r.unfolded)).toEqual([]);
  });
  it('keeps them while only a finer layer closes', () => {
    const t = buildTree([
      { id: 'p', parent: null, kind: 'package', name: 'p' },
      { id: 'q', parent: 'p', kind: 'package', name: 'q' },
      { id: 'A', parent: 'p', kind: 'type', name: 'A' },
      { id: 'B', parent: 'q', kind: 'type', name: 'B' },
      { id: 'f', parent: 'B', kind: 'function', name: 'f' },
    ]);
    const r = shallower(t, { expanded: new Set(['p', 'q', 'B']), unfolded: new Set(['p']) });
    expect(sorted(r.expanded)).toEqual(['p', 'q']);
    expect(sorted(r.unfolded)).toEqual(['p']);
  });
});

describe('stepBack with the kind fold', () => {
  it('restores the expanded and the unfolded set of the last "+"', () => {
    const before = { expanded: new Set(['m', 'p', 'p.q']), unfolded: new Set<string>() };
    const after = { expanded: new Set(['m', 'p', 'p.q', 'p.q.r', 'p.s']), unfolded: new Set(['p', 'p.q']) };
    const r = stepBack(nested, stepForward([], before, after), after);
    expect(r.depth).toEqual(before);
    expect(r.steps).toEqual([]);
  });
});

describe('stepBack', () => {
  it('undoes the last "+" while the state is what it gave, which the leaf rule cannot', () => {
    const before = only('m1', 'p2');
    const after = deeper(tree, { nodes: [], expandable: ['m1', 'p1', 'p2'] }, before);
    expect(sorted(shallower(tree, after).expanded)).toEqual(['m1']);
    const r = stepBack(tree, [{ before, after }], after);
    expect(sorted(r.depth.expanded)).toEqual(['m1', 'p2']);
    expect(r.steps).toEqual([]);
  });
  it('drops a stale history and uses the leaf rule', () => {
    const step = { before: only('m1'), after: only('m1', 'p1') };
    const r = stepBack(tree, [step], only('m1', 'p2'));
    expect(sorted(r.depth.expanded)).toEqual(['m1']);
    expect(r.steps).toEqual([]);
  });
});

describe('stepForward', () => {
  it('pushes the step onto a history that ends at its start, else starts a new one', () => {
    const a = only();
    const b = only('m1');
    const c = only('m1', 'p1', 'p2');
    const one = stepForward([], a, b);
    expect(stepForward(one, b, c)).toEqual([{ before: a, after: b }, { before: b, after: c }]);
    expect(stepForward(one, only('m2'), c)).toEqual([{ before: only('m2'), after: c }]);
    expect(stepForward(one, { expanded: new Set(['m1']), unfolded: new Set(['m1']) }, c)).toHaveLength(1);
  });
});

describe('mustConfirm', () => {
  it('asks above the box limit unless the same step was asked already', () => {
    expect(mustConfirm(BOX_LIMIT, false)).toBe(false);
    expect(mustConfirm(BOX_LIMIT + 1, false)).toBe(true);
    expect(mustConfirm(BOX_LIMIT + 1, true)).toBe(false);
  });
});
