export interface Transform {
  x: number;
  y: number;
  k: number;
}
interface Size {
  width: number;
  height: number;
}

export const MAX_FIT_SCALE = 1.5;

/** Screen = world * k + (x, y). */
export function fitTransform(content: Size, viewport: Size, margin: number): Transform {
  const availW = Math.max(1, viewport.width - 2 * margin);
  const availH = Math.max(1, viewport.height - 2 * margin);
  const k = Math.min(MAX_FIT_SCALE, availW / Math.max(1, content.width), availH / Math.max(1, content.height));
  return {
    k,
    x: (viewport.width - content.width * k) / 2,
    y: (viewport.height - content.height * k) / 2,
  };
}

export function polylineMidpoint(pts: { x: number; y: number }[]): { x: number; y: number } {
  const seg = pts.slice(1).map((p, i) => Math.hypot(p.x - pts[i].x, p.y - pts[i].y));
  let rest = seg.reduce((a, b) => a + b, 0) / 2;
  for (let i = 0; i < seg.length; i++) {
    if (rest <= seg[i] && seg[i] > 0) {
      const f = rest / seg[i];
      return { x: pts[i].x + (pts[i + 1].x - pts[i].x) * f, y: pts[i].y + (pts[i + 1].y - pts[i].y) * f };
    }
    rest -= seg[i];
  }
  return pts[pts.length - 1];
}

export function zoomAt(t: Transform, px: number, py: number, factor: number): Transform {
  const k = t.k * factor;
  return { k, x: px - (px - t.x) * factor, y: py - (py - t.y) * factor };
}

/** Middle of the longest segment; parallel routes are farthest apart there. */
export function labelAnchor(pts: { x: number; y: number }[]): { x: number; y: number; horizontal: boolean } {
  let best = 0;
  let len = -1;
  for (let i = 0; i + 1 < pts.length; i++) {
    const l = Math.hypot(pts[i + 1].x - pts[i].x, pts[i + 1].y - pts[i].y);
    if (l > len) [best, len] = [i, l];
  }
  const a = pts[best];
  const b = pts[Math.min(best + 1, pts.length - 1)];
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, horizontal: Math.abs(b.x - a.x) >= Math.abs(b.y - a.y) };
}
