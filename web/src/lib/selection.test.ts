import { describe, expect, it } from 'vitest';
import { buildTree } from './expansion';
import { lineStrength, openOrClose, pick } from './selection';
import type { GraphNode } from './types';

describe('pick', () => {
  it('selects one box with a plain click and nothing with a click on the background', () => {
    expect(pick(['a', 'b'], 'c', false)).toEqual(['c']);
    expect(pick(['a', 'b'], null, false)).toEqual([]);
  });
  it('adds a box with Shift+click and removes a selected one', () => {
    expect(pick(['a'], 'b', true)).toEqual(['a', 'b']);
    expect(pick(['a', 'b'], 'a', true)).toEqual(['b']);
    expect(pick(['a'], null, true)).toEqual(['a']);
  });
});

describe('lineStrength', () => {
  const ab = { from: 'a', to: 'b' };
  it('gives each line full strength without a selection', () => {
    expect(lineStrength([], ab, false)).toBe('full');
  });
  it('gives the lines of one selected box full strength and dims the others', () => {
    expect(lineStrength(['a'], ab, false)).toBe('full');
    expect(lineStrength(['b'], ab, true)).toBe('full');
    expect(lineStrength(['c'], ab, false)).toBe('dim');
  });
  it('with some boxes: full between them, weak to the outside, dim elsewhere', () => {
    expect(lineStrength(['a', 'b'], ab, false)).toBe('full');
    expect(lineStrength(['a', 'c'], ab, false)).toBe('weak');
    expect(lineStrength(['c', 'd'], ab, false)).toBe('dim');
  });
  it('with between only, dims the lines to the outside', () => {
    expect(lineStrength(['a', 'b'], ab, true)).toBe('full');
    expect(lineStrength(['a', 'c'], ab, true)).toBe('dim');
  });
});

describe('openOrClose', () => {
  const nodes: GraphNode[] = [
    { id: 'm', parent: null, kind: 'module', name: 'm' },
    { id: 'p', parent: 'm', kind: 'package', name: 'p' },
    { id: 'q', parent: 'm', kind: 'package', name: 'q' },
    { id: 't', parent: 'p', kind: 'type', name: 'T' },
    { id: 'u', parent: 'q', kind: 'type', name: 'U' },
    { id: 'f', parent: 't', kind: 'function', name: 'f' },
  ];
  const tree = buildTree(nodes);
  it('opens each selected container that can open', () => {
    expect(openOrClose(tree, new Set(['m']), ['p', 'q'], ['p', 'q', 'u'])).toEqual(new Set(['m', 'p', 'q']));
  });
  it('closes the open selected containers when none is left to open', () => {
    expect(openOrClose(tree, new Set(['m', 'p', 't', 'q']), ['u'], ['p', 'q'])).toEqual(new Set(['m']));
  });
  it('gives null when no selected box can open or close', () => {
    expect(openOrClose(tree, new Set(['m']), ['p', 'q'], ['u'])).toBeNull();
    expect(openOrClose(tree, new Set(['m']), ['p', 'q'], [])).toBeNull();
  });
});
