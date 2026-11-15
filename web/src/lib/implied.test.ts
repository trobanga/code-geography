import { describe, expect, it } from 'vitest';
import { impliedLines, impliedSegments } from './implied';

const l = (from: string, to: string) => ({ from, to });
const none = () => false;
/** The hidden lines as `from>to`, sorted. */
const hidden = (lines: { from: string; to: string }[], isProtected: (e: { from: string; to: string }) => boolean = none) =>
  [...impliedLines(lines, isProtected)].map((i) => `${lines[i].from}>${lines[i].to}`).sort();

describe('impliedLines', () => {
  it('hides the chord of a chain', () => {
    expect(hidden([l('a', 'b'), l('b', 'c'), l('a', 'c')])).toEqual(['a>c']);
  });

  it('keeps the four sides of a diamond and hides its diagonal', () => {
    expect(hidden([l('a', 'b'), l('a', 'c'), l('b', 'd'), l('c', 'd')])).toEqual([]);
    expect(hidden([l('a', 'b'), l('a', 'c'), l('b', 'd'), l('c', 'd'), l('a', 'd')])).toEqual(['a>d']);
  });

  it('keeps both lines of a two-node cycle to a common target', () => {
    expect(hidden([l('a', 'b'), l('b', 'a'), l('a', 'c'), l('b', 'c')])).toEqual([]);
  });

  it('hides a chord into a cycle that a path reaches, and no line inside the cycle', () => {
    // x → y → a ⇄ b, and the chord x → b.
    const lines = [l('x', 'y'), l('y', 'a'), l('a', 'b'), l('b', 'a'), l('x', 'b')];
    expect(hidden(lines)).toEqual(['x>b']);
    // Two lines into the same cycle with no component between them both stay.
    expect(hidden([l('x', 'a'), l('a', 'b'), l('b', 'a'), l('x', 'b')])).toEqual([]);
  });

  it('never hides a protected line, but follows it for paths', () => {
    const lines = [l('a', 'b'), l('b', 'c'), l('a', 'c'), l('c', 'd'), l('a', 'd')];
    const isProtected = (e: { from: string; to: string }) => e.from === 'a' && e.to === 'c';
    // a → c stays; a → d is still implied by a → b → c → d.
    expect(hidden(lines, isProtected)).toEqual(['a>d']);
  });

  it('treats parallel lines as one edge and hides each parallel chord', () => {
    expect(hidden([l('a', 'b'), l('a', 'b'), l('b', 'c')])).toEqual([]);
    expect(hidden([l('a', 'b'), l('b', 'c'), l('a', 'c'), l('a', 'c')])).toEqual(['a>c', 'a>c']);
  });

  it('hides nothing in an empty input', () => {
    expect(impliedLines([], none).size).toBe(0);
  });

  it('reduces a 500-node chain with all chords to the chain, fast', () => {
    const n = 500;
    const lines: { from: string; to: string }[] = [];
    for (let i = 0; i < n; i++) for (let j = i + 1; j < n; j++) lines.push(l(`n${i}`, `n${j}`));
    const t0 = performance.now();
    const h = impliedLines(lines, none);
    const ms = performance.now() - t0;
    expect(lines.length - h.size).toBe(n - 1);
    expect([...lines.keys()].filter((i) => !h.has(i)).every((i) => Number(lines[i].to.slice(1)) === Number(lines[i].from.slice(1)) + 1)).toBe(true);
    expect(ms).toBeLessThan(1000);
  });
});

describe('impliedSegments', () => {
  const rects = new Map([
    ['a', { x: 0, y: 0, w: 10, h: 10 }],
    ['b', { x: 100, y: 0, w: 10, h: 10 }],
    ['c', { x: 0, y: 100, w: 10, h: 10 }],
  ]);
  it('draws the hidden lines in and out of the selected box as straight border-to-border segments', () => {
    const lines = [l('a', 'b'), l('c', 'a'), l('b', 'c')];
    const s = impliedSegments(lines, ['a'], rects);
    expect(s.map((x) => `${x.line.from}>${x.line.to}`)).toEqual(['a>b', 'c>a']);
    expect(s[0].points).toEqual([
      { x: 10, y: 5 },
      { x: 100, y: 5 },
    ]);
    expect(s[1].points).toEqual([
      { x: 5, y: 100 },
      { x: 5, y: 10 },
    ]);
  });
  it('draws each hidden line of some selected boxes one time', () => {
    const s = impliedSegments([l('a', 'b'), l('c', 'a'), l('b', 'c')], ['a', 'b'], rects);
    expect(s.map((x) => `${x.line.from}>${x.line.to}`)).toEqual(['a>b', 'c>a', 'b>c']);
  });
  it('draws nothing without a selection or for a box not laid out', () => {
    expect(impliedSegments([l('a', 'b')], [], rects)).toEqual([]);
    expect(impliedSegments([l('a', 'x')], ['a'], rects)).toEqual([]);
  });
});
