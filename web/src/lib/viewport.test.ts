import { describe, expect, it } from 'vitest';
import { fitTransform, labelAnchor, polylineMidpoint, zoomAt } from './viewport';

describe('fitTransform', () => {
  it('scales the content into the viewport with a margin and centers it', () => {
    const t = fitTransform({ width: 1000, height: 500 }, { width: 520, height: 520 }, 10);
    expect(t.k).toBe(0.5);
    expect(t.x).toBe(10);
    expect(t.y).toBe(135);
  });
  it('never scales above 1.5', () => {
    expect(fitTransform({ width: 10, height: 10 }, { width: 1000, height: 1000 }, 0).k).toBe(1.5);
  });
});

describe('polylineMidpoint', () => {
  it('returns the point at half the arc length', () => {
    const pts = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 30 },
    ];
    expect(polylineMidpoint(pts)).toEqual({ x: 10, y: 10 });
  });
});

describe('zoomAt', () => {
  it('keeps the point under the pointer fixed', () => {
    const t0 = { x: 30, y: -20, k: 0.5 };
    const t1 = zoomAt(t0, 100, 80, 2);
    const world = (t: typeof t0) => [(100 - t.x) / t.k, (80 - t.y) / t.k];
    expect(t1.k).toBe(1);
    expect(world(t1)).toEqual(world(t0));
  });
});

describe('labelAnchor', () => {
  it('puts the label on the middle of the longest segment', () => {
    const a = labelAnchor([
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 100 },
      { x: 40, y: 100 },
    ]);
    expect(a).toEqual({ x: 10, y: 50, horizontal: false });
  });

  it('marks horizontal segments', () => {
    expect(labelAnchor([{ x: 0, y: 5 }, { x: 80, y: 5 }])).toEqual({ x: 40, y: 5, horizontal: true });
  });
});
