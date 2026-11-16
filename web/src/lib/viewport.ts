export interface Transform {
  x: number;
  y: number;
  k: number;
}
interface Size {
  width: number;
  height: number;
}

interface Box extends Size {
  x: number;
  y: number;
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

/**
 * The transform that shows `box` (world coordinates) inside the viewport, `margin` pixels from
 * its edges, with the least change: `t` itself when the box is in view.
 */
export function reveal(box: Box, viewport: Size, t: Transform, margin: number): Transform {
  // The scale rule of `fitTransform`, capped by the present scale: never zooms in.
  const fit = fitTransform(box, viewport, margin);
  if (fit.k < t.k) {
    return {
      k: fit.k,
      x: viewport.width / 2 - (box.x + box.width / 2) * fit.k,
      y: viewport.height / 2 - (box.y + box.height / 2) * fit.k,
    };
  }
  return panTo(box, viewport, t, margin);
}

/**
 * Like `reveal`, but never changes the scale: `t` itself also when the box does not fit into
 * the viewport. For a viewport that changed its size.
 */
export function keepInView(box: Box, viewport: Size, t: Transform, margin: number): Transform {
  const fits = (size: number, room: number) => size * t.k <= room - 2 * margin;
  return fits(box.width, viewport.width) && fits(box.height, viewport.height) ? panTo(box, viewport, t, margin) : t;
}

/** The least pan that brings a box that fits into the viewport inside its margins. */
function panTo(box: Box, viewport: Size, t: Transform, margin: number): Transform {
  const left = box.x * t.k + t.x;
  const top = box.y * t.k + t.y;
  const dx = shift(left, left + box.width * t.k, viewport.width, margin);
  const dy = shift(top, top + box.height * t.k, viewport.height, margin);
  if (dx === 0 && dy === 0) return t;
  return { k: t.k, x: t.x + dx, y: t.y + dy };
}

/** Pan along one axis that brings the screen span `lo`..`hi` inside 0..`size` less the margins. */
function shift(lo: number, hi: number, size: number, margin: number): number {
  if (lo < margin) return margin - lo;
  if (hi > size - margin) return size - margin - hi;
  return 0;
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
