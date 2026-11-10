import { describe, expect, it } from 'vitest';
import { BOX_LIMIT, buildTree, mustConfirm, collapse, deeper, drawable, shallower, stepBack, stepForward, levelExpanded, splitByLevel } from './expansion';
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

describe('levelExpanded', () => {
  it('modules: nothing expanded', () => {
    expect(sorted(levelExpanded(tree, 'modules'))).toEqual([]);
  });
  it('packages: modules with children expanded', () => {
    expect(sorted(levelExpanded(tree, 'packages'))).toEqual(['m1']);
  });
  it('types: modules and packages with children expanded', () => {
    expect(sorted(levelExpanded(tree, 'types'))).toEqual(['m1', 'p1', 'p2']);
  });
  it('functions: modules, packages and types (nested too) with children expanded', () => {
    expect(sorted(levelExpanded(tree, 'functions'))).toEqual(['m1', 'p1', 'p2', 't1', 't1i']);
  });
});

describe('splitByLevel', () => {
  it('finds the deepest contained level and the extra open ids', () => {
    expect(splitByLevel(tree, new Set(['m1', 'p1', 'p2', 't1']))).toEqual({ level: 'types', open: ['t1'] });
  });
  it('falls back to modules when a level node was collapsed', () => {
    expect(splitByLevel(tree, new Set(['p1']))).toEqual({ level: 'modules', open: ['p1'] });
  });
  it('ignores level containers that the view does not show, e.g. ones with only test code', () => {
    const shown = new Set(['m1', 'm2', 'p1', 't1', 't1i', 'f1', 'f2']);
    expect(splitByLevel(tree, new Set(['m1', 'p1']), shown)).toEqual({ level: 'types', open: [] });
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

describe('deeper', () => {
  it('opens every visible closed container of the view, skips placeholders', () => {
    expect(sorted(deeper(['p1', 'p2', 'rest:m1'], new Set(['m1'])))).toEqual(['m1', 'p1', 'p2']);
  });
});

describe('shallower', () => {
  it('closes every open container without an open descendant', () => {
    expect(sorted(shallower(tree, new Set(['m1', 'p1', 't1', 'p2', 'm2'])))).toEqual(['m1', 'p1']);
  });
  it('drops open ids below a closed container, which the cut does not show', () => {
    expect(sorted(shallower(tree, new Set(['m1', 't1'])))).toEqual([]);
  });
});

describe('stepBack', () => {
  it('undoes the last "+" while the state is what it gave, which the leaf rule cannot', () => {
    const before = new Set(['m1', 'p2']);
    const after = deeper(['m1', 'p1', 'p2'], before);
    expect(sorted(shallower(tree, after))).toEqual(['m1']);
    const r = stepBack(tree, [{ before, after }], after);
    expect(sorted(r.expanded)).toEqual(['m1', 'p2']);
    expect(r.steps).toEqual([]);
  });
  it('drops a stale history and uses the leaf rule', () => {
    const step = { before: new Set(['m1']), after: new Set(['m1', 'p1']) };
    const r = stepBack(tree, [step], new Set(['m1', 'p2']));
    expect(sorted(r.expanded)).toEqual(['m1']);
    expect(r.steps).toEqual([]);
  });
});

describe('stepForward', () => {
  it('pushes the step onto a history that ends at its start, else starts a new one', () => {
    const a = new Set<string>();
    const b = new Set(['m1']);
    const c = new Set(['m1', 'p1', 'p2']);
    const one = stepForward([], a, b);
    expect(stepForward(one, b, c)).toEqual([{ before: a, after: b }, { before: b, after: c }]);
    expect(stepForward(one, new Set(['m2']), c)).toEqual([{ before: new Set(['m2']), after: c }]);
  });
});

describe('mustConfirm', () => {
  it('asks above the box limit unless the same step was asked already', () => {
    expect(mustConfirm(BOX_LIMIT, false)).toBe(false);
    expect(mustConfirm(BOX_LIMIT + 1, false)).toBe(true);
    expect(mustConfirm(BOX_LIMIT + 1, true)).toBe(false);
  });
});
