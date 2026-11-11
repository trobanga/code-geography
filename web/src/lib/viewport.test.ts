import { describe, expect, it } from 'vitest';
import { fitTransform, labelAnchor, polylineMidpoint, reveal, zoomAt } from './viewport';

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

describe('reveal', () => {
  const vp = { width: 1000, height: 600 };
  it('keeps the transform when the box is inside the viewport with the margin', () => {
    const t = { x: 10, y: 20, k: 0.5 };
    expect(reveal({ x: 100, y: 100, width: 200, height: 100 }, vp, t, 16)).toBe(t);
  });
  it('pans the least so that a box off the left edge comes in', () => {
    // Screen left = 100 * 0.5 - 80 = -30: 46 pixels to the margin.
    expect(reveal({ x: 100, y: 100, width: 200, height: 100 }, vp, { x: -80, y: 20, k: 0.5 }, 16)).toEqual({ x: -34, y: 20, k: 0.5 });
  });
  it('pans the least so that a box off the right edge comes in', () => {
    // Screen right = 300 * 0.5 + 900 = 1050: 66 pixels past the margin.
    expect(reveal({ x: 100, y: 100, width: 200, height: 100 }, vp, { x: 900, y: 20, k: 0.5 }, 16)).toEqual({ x: 834, y: 20, k: 0.5 });
  });
  it('pans the least so that a box above or below the viewport comes in', () => {
    const box = { x: 100, y: 100, width: 200, height: 100 };
    // Screen top = 50 - 60 = -10; screen bottom = 100 + 560 = 660.
    expect(reveal(box, vp, { x: 10, y: -60, k: 0.5 }, 16)).toEqual({ x: 10, y: -34, k: 0.5 });
    expect(reveal(box, vp, { x: 10, y: 560, k: 0.5 }, 16)).toEqual({ x: 10, y: 484, k: 0.5 });
  });
  it('scales down just enough and centres a box larger than the viewport', () => {
    // At k = 1 the box is 1936 wide; 1000 - 2 * 16 = 968 fit, so k = 0.5.
    const t = reveal({ x: 100, y: 100, width: 1936, height: 400 }, vp, { x: 0, y: 0, k: 1 }, 16);
    expect(t).toEqual({ k: 0.5, x: 500 - (100 + 968) * 0.5, y: 300 - 300 * 0.5 });
  });
  it('pans a box that is on screen but within the margin', () => {
    const box = { x: 0, y: 100, width: 100, height: 100 };
    const t = { x: 5, y: 0, k: 1 };
    expect(reveal(box, vp, t, 0)).toBe(t);
    expect(reveal(box, vp, t, 16)).toEqual({ x: 16, y: 0, k: 1 });
  });
});
